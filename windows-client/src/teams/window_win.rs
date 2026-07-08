//! `CallSource` that watches for Teams' own on-screen ring/toast window
//! directly, via `SetWinEventHook`, instead of the OS notification pipeline.
//!
//! Why: new Teams (WebView2-based) draws its "Show in Banner" popup as its
//! own always-on-top Win32 window rather than routing it through the real
//! Windows toast/AppNotification system — confirmed empirically (see
//! `toast-log.txt` calibration run: `UserNotificationListener` caught
//! Outlook and system toasts but zero Teams notifications over an hour).
//! `listener_win::ToastCallSource` therefore cannot see Teams calls at all
//! on current Teams versions. This source reads the actual rendered window
//! instead, which exists regardless of whether Teams registers with the OS
//! toast pipeline.
//!
//! KNOWN LIMITATION (from calibration on a real box): the ring popup is a
//! `WS_EX_TOPMOST` `TeamsWebView` window (log tag `Notifications`) whose
//! visible content — including the caller name — is rendered inside a
//! WebView2 (`Chrome_WidgetWin_*` / `Chrome_RenderWidgetHostHWND` children).
//! `GetWindowText` on those children returns EMPTY (WebView2 paints to the
//! GPU, not window titles), so `collect_text_lines` here yields nothing and
//! this source cannot currently detect a call by window text alone. Reading
//! the caller would require full UI Automation (`IUIAutomation` +
//! `ElementFromHandle`, walking the WebView2 provider tree) — left as future
//! work. Until then the reliable detector is `logtail::LogTailCallSource`
//! (the default `TEAMS_CALL_SOURCE`), which matches a confirmed call-start
//! marker in Teams' own log. Recalibrate with `cargo run --bin window_logger`
//! if a future Teams build changes this.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumChildWindows, GetMessageW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, TranslateMessage, EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, MSG,
    WINEVENT_OUTOFCONTEXT,
};

use super::classify::CallClassifier;
use super::source::CallSource;

/// New Teams' process image name. Classic Teams (`Teams.exe`) is being
/// retired; both are matched so the source still works during the interim.
const TEAMS_PROCESS_NAMES: &[&str] = &["ms-teams.exe", "teams.exe"];

// The hook callback runs on the thread that called `SetWinEventHook` and
// has no way to carry a closure's captured state through the raw win32
// callback pointer, so the sender and classifier are stashed here instead.
static SINK: Mutex<Option<(Sender<super::classify::IncomingCall>, CallClassifier)>> =
    Mutex::new(None);

pub struct WindowCallSource {
    classifier: CallClassifier,
}

impl WindowCallSource {
    pub fn new(classifier: CallClassifier) -> Self {
        WindowCallSource { classifier }
    }
}

fn process_name_for_window(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn is_teams_window(hwnd: HWND) -> bool {
    process_name_for_window(hwnd)
        .map(|path| {
            let lower = path.to_lowercase();
            TEAMS_PROCESS_NAMES.iter().any(|name| lower.ends_with(name))
        })
        .unwrap_or(false)
}

fn window_text(hwnd: HWND) -> Option<String> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..copied as usize]))
    }
}

/// Collects the title text of `hwnd` and every descendant child window —
/// approximates a UIA text-tree walk without pulling in the full
/// `IUIAutomation` COM surface. Good enough for WebView2-hosted content
/// where visible text often ends up on child window titles/accessible
/// names; revisit with real `IUIAutomation` if `window_logger` calibration
/// shows this misses the caller name.
fn collect_text_lines(hwnd: HWND) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(t) = window_text(hwnd) {
        if !t.trim().is_empty() {
            lines.push(t);
        }
    }

    unsafe extern "system" fn enum_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let lines = &mut *(lparam.0 as *mut Vec<String>);
        if let Some(t) = window_text(hwnd) {
            if !t.trim().is_empty() {
                lines.push(t);
            }
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(enum_child),
            LPARAM(&mut lines as *mut _ as isize),
        );
    }
    lines
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread_id: u32,
    _time: u32,
) {
    if hwnd.0.is_null() || !is_teams_window(hwnd) {
        return;
    }

    let lines = collect_text_lines(hwnd);
    if lines.is_empty() {
        return;
    }

    if let Ok(guard) = SINK.lock() {
        if let Some((tx, classifier)) = guard.as_ref() {
            if let Some(call) = classifier.classify_lines(&lines) {
                let _ = tx.send(call);
            }
        }
    }
}

impl CallSource for WindowCallSource {
    fn start(self) -> Receiver<super::classify::IncomingCall> {
        let (tx, rx) = mpsc::channel();
        *SINK.lock().expect("SINK mutex poisoned") = Some((tx, self.classifier));

        thread::spawn(move || unsafe {
            let hook = SetWinEventHook(
                EVENT_OBJECT_CREATE,
                EVENT_OBJECT_SHOW,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if hook.is_invalid() {
                tracing::error!("SetWinEventHook failed for Teams window watcher");
                return;
            }

            // WinEvent hooks deliver via the thread's message queue — needs
            // a real message pump, not a sleep loop.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let _ = UnhookWinEvent(hook);
        });

        rx
    }
}
