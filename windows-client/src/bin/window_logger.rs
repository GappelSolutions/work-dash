//! Calibration tool for `teams::window_win`: logs every window Teams creates
//! or shows (class name, title, child-window text) for a fixed duration.
//! Run this on the real Windows machine, trigger a real incoming call and a
//! chat toast, then grep the log for the ring popup's window signature —
//! that signature is what `window_win::is_teams_window`/`collect_text_lines`
//! need to match reliably. Unlike `toast_logger`, this does NOT need
//! package identity — `SetWinEventHook` works from a plain unpackaged
//! process.
//!
//! Env vars:
//!   WINDOW_LOG_PATH          default "window-log.txt"
//!   WINDOW_LOG_DURATION_SECS default 3600 (1 hour)

#[cfg(windows)]
fn main() {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, EnumChildWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, PeekMessageW, TranslateMessage, EVENT_OBJECT_CREATE,
        EVENT_OBJECT_SHOW, MSG, PM_REMOVE, WINEVENT_OUTOFCONTEXT,
    };

    let args: Vec<String> = std::env::args().collect();
    let log_path = std::env::var("WINDOW_LOG_PATH")
        .ok()
        .or_else(|| args.get(1).cloned())
        .unwrap_or_else(|| "window-log.txt".to_string());
    let duration_secs: u64 = std::env::var("WINDOW_LOG_DURATION_SECS")
        .ok()
        .or_else(|| args.get(2).cloned())
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600);

    println!("logging Teams window activity to {log_path} for {duration_secs}s");

    static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("open log file");
    let header = format!("=== run start {}\n", chrono::Utc::now().to_rfc3339());
    file.write_all(header.as_bytes()).expect("write header");
    *LOG.lock().unwrap() = Some(file);

    fn process_image_name(hwnd: HWND) -> String {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return "<no-pid>".to_string();
            }
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return format!("<open-process-failed pid={pid}>");
            };
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_err()
            {
                return format!("<query-name-failed pid={pid}>");
            }
            String::from_utf16_lossy(&buf[..len as usize])
        }
    }

    fn window_text(hwnd: HWND) -> String {
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len as usize + 1];
            let copied = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..copied.max(0) as usize])
        }
    }

    fn class_name(hwnd: HWND) -> String {
        unsafe {
            let mut buf = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..len.max(0) as usize])
        }
    }

    unsafe extern "system" fn enum_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let lines = &mut *(lparam.0 as *mut Vec<String>);
        let text = window_text(hwnd);
        let class = class_name(hwnd);
        if !text.trim().is_empty() || !class.is_empty() {
            lines.push(format!("    child class={class:?} text={text:?}"));
        }
        BOOL(1)
    }

    unsafe extern "system" fn win_event_proc(
        _hook: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _thread_id: u32,
        _time: u32,
    ) {
        if hwnd.0.is_null() {
            return;
        }
        let image = process_image_name(hwnd);
        let lower = image.to_lowercase();
        if !lower.contains("teams") {
            return;
        }

        let event_name = if event == EVENT_OBJECT_CREATE {
            "CREATE"
        } else {
            "SHOW"
        };
        let mut lines = vec![format!(
            "{} event={event_name} process={image:?} class={:?} title={:?}",
            chrono::Utc::now().to_rfc3339(),
            class_name(hwnd),
            window_text(hwnd)
        )];
        let mut child_lines = Vec::new();
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(enum_child),
            LPARAM(&mut child_lines as *mut _ as isize),
        );
        lines.extend(child_lines);
        lines.push(String::new());

        if let Ok(mut guard) = LOG.lock() {
            if let Some(file) = guard.as_mut() {
                let entry = lines.join("\n") + "\n";
                print!("{entry}");
                let _ = file.write_all(entry.as_bytes());
                let _ = file.flush();
            }
        }
    }

    unsafe {
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
            eprintln!("SetWinEventHook failed");
            std::process::exit(1);
        }

        let deadline = Instant::now() + Duration::from_secs(duration_secs);
        let mut msg = MSG::default();
        while Instant::now() < deadline {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let _ = UnhookWinEvent(hook);
    }

    println!("done — log written to {log_path}");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("window_logger only works on Windows (needs SetWinEventHook).");
    std::process::exit(1);
}
