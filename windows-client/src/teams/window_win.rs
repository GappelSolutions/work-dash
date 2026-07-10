//! `CallSource` that detects an incoming Teams call by inspecting Teams' own
//! ring popup window via UI Automation (UIA), instead of the OS notification
//! pipeline (which new Teams bypasses) or the log (which carries no reliable,
//! headset-independent call marker — a full workday of capture produced zero
//! call-specific log lines without a Bluetooth/HFP headset connected).
//!
//! How it works:
//!   1. `SetWinEventHook` watches for windows becoming visible.
//!   2. We filter to Teams' notification popup: a top-level `TeamsWebView`
//!      window with the `WS_EX_TOPMOST` extended style (the main Teams window
//!      is also `TeamsWebView` but is not topmost, so this excludes it).
//!   3. The HWND is handed to a worker thread that (with COM initialised)
//!      reads the window's UIA subtree. The popup's content is a WebView2,
//!      whose accessibility tree exposes the rendered controls — crucially,
//!      an incoming call shows **Accept** and **Decline** buttons, which a
//!      chat/mention toast never does. Matching those control names (EN + DE)
//!      is what distinguishes a call from any other toast.
//!
//! The UIA tree isn't populated the instant the window shows, so the worker
//! retries for a short window before giving up.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, SetWinEventHook, UnhookWinEvent,
    HWINEVENTHOOK, TreeScope_Descendants,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetClassNameW, GetMessageW, GetWindowLongPtrW, GetWindowThreadProcessId,
    TranslateMessage, EVENT_OBJECT_SHOW, GWL_EXSTYLE, MSG, WINEVENT_OUTOFCONTEXT, WS_EX_TOPMOST,
};

use super::classify::{CallClassifier, IncomingCall};
use super::source::CallSource;

/// New Teams' process image name. Classic Teams (`Teams.exe`) is being
/// retired; both are matched so the source still works during the interim.
const TEAMS_PROCESS_NAMES: &[&str] = &["ms-teams.exe", "teams.exe"];

/// The ring popup is a `TeamsWebView` window; so is the main window, but only
/// the popup is `WS_EX_TOPMOST`.
const NOTIFICATION_WINDOW_CLASS: &str = "TeamsWebView";

/// UIA control names (lowercased) that only an incoming-call ring shows.
/// Chat/mention toasts expose "Reply"/"Antworten" instead, never these.
/// German included — the target user's Teams UI is German.
const CALL_DECLINE_NAMES: &[&str] = &["decline", "ablehnen", "reject"];
const CALL_ACCEPT_NAMES: &[&str] = &[
    "accept",
    "annehmen",
    "akzeptieren",
    "accept with audio",
    "accept with video",
    "mit audio",
    "mit video",
];

/// Names we should never treat as the caller (they're the action buttons).
const BUTTON_NAMES: &[&str] = &[
    "decline",
    "ablehnen",
    "reject",
    "accept",
    "annehmen",
    "akzeptieren",
    "audio",
    "video",
    "mit audio",
    "mit video",
];

// The raw win32 hook callback can't carry captured state, so the channel the
// hook uses to hand HWNDs to the UIA worker is stashed here.
static HWND_TX: Mutex<Option<Sender<isize>>> = Mutex::new(None);

pub struct WindowCallSource {
    classifier: CallClassifier,
}

impl WindowCallSource {
    pub fn new(classifier: CallClassifier) -> Self {
        // classifier is retained for API symmetry with the other sources /
        // possible future phrase-based caller filtering; call detection here
        // is structural (Accept/Decline controls), not phrase-based.
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

fn is_teams_process(hwnd: HWND) -> bool {
    process_name_for_window(hwnd)
        .map(|path| {
            let lower = path.to_lowercase();
            TEAMS_PROCESS_NAMES.iter().any(|name| lower.ends_with(name))
        })
        .unwrap_or(false)
}

fn class_name(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }
}

fn is_topmost(hwnd: HWND) -> bool {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        ex & WS_EX_TOPMOST.0 != 0
    }
}

/// Cheap pre-filter (no UIA) run in the hook callback: is this plausibly the
/// Teams ring/notification popup worth a UIA inspection?
fn looks_like_notification_window(hwnd: HWND) -> bool {
    !hwnd.0.is_null()
        && class_name(hwnd) == NOTIFICATION_WINDOW_CLASS
        && is_topmost(hwnd)
        && is_teams_process(hwnd)
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread_id: u32,
    _time: u32,
) {
    // OBJID_WINDOW == 0: only whole-window show events, not child objects.
    if id_object != 0 {
        return;
    }
    if !looks_like_notification_window(hwnd) {
        return;
    }
    if let Ok(guard) = HWND_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(hwnd.0 as isize);
        }
    }
}

/// Reads every UIA element name under `hwnd`. Returns `None` if UIA can't
/// reach the window yet.
fn read_uia_names(automation: &IUIAutomation, hwnd: HWND) -> Option<Vec<String>> {
    unsafe {
        let element: IUIAutomationElement = automation.ElementFromHandle(hwnd).ok()?;
        let condition = automation.CreateTrueCondition().ok()?;
        let all = element.FindAll(TreeScope_Descendants, &condition).ok()?;
        let len = all.Length().ok()?;
        let mut names = Vec::new();
        for i in 0..len {
            if let Ok(e) = all.GetElement(i) {
                if let Ok(name) = e.CurrentName() {
                    let s = name.to_string();
                    if !s.trim().is_empty() {
                        names.push(s);
                    }
                }
            }
        }
        Some(names)
    }
}

/// Decides whether the collected UIA names describe an incoming call, and if
/// so extracts a best-effort caller. Pure — unit-tested.
fn classify_call(names: &[String]) -> Option<IncomingCall> {
    let lower: Vec<String> = names.iter().map(|s| s.to_lowercase()).collect();

    let has_decline = lower
        .iter()
        .any(|n| CALL_DECLINE_NAMES.iter().any(|d| n.contains(d)));
    let has_accept = lower
        .iter()
        .any(|n| CALL_ACCEPT_NAMES.iter().any(|a| n.contains(a)));

    // A ring shows both Accept and Decline. Requiring both avoids matching a
    // stray "Decline"/"Accept" label elsewhere in the UI.
    if !(has_decline && has_accept) {
        return None;
    }

    // Best-effort caller: the first name that isn't an action button and
    // isn't obviously boilerplate. Teams typically renders the caller name
    // prominently in the toast; fall back if we can't isolate it.
    let caller = names
        .iter()
        .map(|s| s.trim())
        .find(|s| {
            let low = s.to_lowercase();
            !s.is_empty()
                && !BUTTON_NAMES.iter().any(|b| low.contains(b))
                && !low.contains("microsoft teams")
                && !low.contains("incoming")
                && !low.contains("calling")
                && !low.contains("eingehend") // DE "incoming"
                && s.len() > 1
        })
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Teams call".to_string());

    Some(IncomingCall { caller })
}

impl CallSource for WindowCallSource {
    fn start(self) -> Receiver<IncomingCall> {
        let (call_tx, call_rx) = mpsc::channel::<IncomingCall>();
        let (hwnd_tx, hwnd_rx) = mpsc::channel::<isize>();
        *HWND_TX.lock().expect("HWND_TX mutex poisoned") = Some(hwnd_tx);
        let _ = &self.classifier;

        // Worker thread: owns COM + UIA, inspects each candidate window.
        thread::spawn(move || unsafe {
            if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                tracing::error!("CoInitializeEx failed in Teams UIA worker");
                return;
            }
            let automation: IUIAutomation =
                match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!(?e, "failed to create UIAutomation instance");
                        return;
                    }
                };

            for raw in hwnd_rx {
                let hwnd = HWND(raw as *mut _);
                // The WebView2 accessibility tree lags the window's show by a
                // beat; retry briefly before giving up.
                let mut emitted = false;
                for _ in 0..10 {
                    if let Some(names) = read_uia_names(&automation, hwnd) {
                        if let Some(call) = classify_call(&names) {
                            if call_tx.send(call).is_err() {
                                return;
                            }
                            emitted = true;
                            break;
                        }
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                let _ = emitted;
            }
        });

        // Hook thread: message pump so WinEvent callbacks fire.
        thread::spawn(move || unsafe {
            let hook = SetWinEventHook(
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_SHOW,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if hook.is_invalid() {
                tracing::error!("SetWinEventHook failed for Teams ring watcher");
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWinEvent(hook);
        });

        call_rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_and_decline_present_is_a_call() {
        let names = vec![
            "Sarah Lee".to_string(),
            "Incoming call".to_string(),
            "Accept".to_string(),
            "Decline".to_string(),
        ];
        assert_eq!(
            classify_call(&names),
            Some(IncomingCall {
                caller: "Sarah Lee".to_string()
            })
        );
    }

    #[test]
    fn german_accept_decline_is_a_call() {
        let names = vec![
            "Max Mustermann".to_string(),
            "Eingehender Anruf".to_string(),
            "Annehmen".to_string(),
            "Ablehnen".to_string(),
        ];
        assert_eq!(
            classify_call(&names),
            Some(IncomingCall {
                caller: "Max Mustermann".to_string()
            })
        );
    }

    #[test]
    fn chat_toast_with_only_reply_is_not_a_call() {
        let names = vec![
            "Sarah Lee".to_string(),
            "Hey are you around?".to_string(),
            "Reply".to_string(),
        ];
        assert_eq!(classify_call(&names), None);
    }

    #[test]
    fn decline_without_accept_is_not_a_call() {
        // Guards against a stray "Decline"-like label elsewhere.
        let names = vec!["Decline meeting".to_string()];
        assert_eq!(classify_call(&names), None);
    }

    #[test]
    fn caller_falls_back_when_only_buttons_present() {
        let names = vec!["Accept".to_string(), "Decline".to_string()];
        assert_eq!(
            classify_call(&names),
            Some(IncomingCall {
                caller: "Teams call".to_string()
            })
        );
    }
}
