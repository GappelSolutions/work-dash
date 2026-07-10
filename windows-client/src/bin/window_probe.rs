//! Calibration probe for `teams::window_win`. Watches for Teams' ring/toast
//! popup and, when one appears, dumps the UIA **Button** control names inside
//! it plus a control-type histogram — then prints the detector's verdict.
//!
//! Privacy: it prints only Button-type element names (UI chrome like
//! Accept/Decline) and never Text-type elements, so a caller's name is not
//! shown. Run it, place a Teams call to yourself, and share the output so the
//! Accept/Decline label list in `window_win` can be confirmed/adjusted.
//!
//! Usage: cargo run --bin window_probe   (Ctrl+C to stop)

#[cfg(windows)]
fn main() {
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, SetWinEventHook, HWINEVENTHOOK, TreeScope_Descendants,
        UIA_ButtonControlTypeId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetClassNameW, GetMessageW, GetWindowLongPtrW, GetWindowThreadProcessId,
        TranslateMessage, EVENT_OBJECT_SHOW, GWL_EXSTYLE, MSG, WINEVENT_OUTOFCONTEXT,
        WS_EX_TOPMOST,
    };

    static HWND_TX: std::sync::Mutex<Option<mpsc::Sender<isize>>> = std::sync::Mutex::new(None);

    fn class_name(hwnd: HWND) -> String {
        unsafe {
            let mut buf = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..len.max(0) as usize])
        }
    }

    fn is_teams(hwnd: HWND) -> bool {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return false;
            }
            let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_err() {
                return false;
            }
            String::from_utf16_lossy(&buf[..len as usize])
                .to_lowercase()
                .contains("teams")
        }
    }

    unsafe extern "system" fn cb(
        _h: HWINEVENTHOOK,
        _e: u32,
        hwnd: HWND,
        id_object: i32,
        _c: i32,
        _t: u32,
        _tm: u32,
    ) {
        if id_object != 0 || hwnd.0.is_null() {
            return;
        }
        if class_name(hwnd) != "TeamsWebView" {
            return;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOPMOST.0 == 0 || !is_teams(hwnd) {
            return;
        }
        if let Ok(g) = HWND_TX.lock() {
            if let Some(tx) = g.as_ref() {
                let _ = tx.send(hwnd.0 as isize);
            }
        }
    }

    let (tx, rx) = mpsc::channel::<isize>();
    *HWND_TX.lock().unwrap() = Some(tx);

    println!("probe running — place a Teams call to yourself. Ctrl+C to stop.");
    println!("(only Button labels are printed; caller-name Text elements are not)");

    // Worker: COM + UIA inspection.
    thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            eprintln!("CoInitializeEx failed");
            return;
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).expect("UIAutomation");

        for raw in rx {
            let hwnd = HWND(raw as *mut _);
            println!("\n=== notification window shown (hwnd={raw:#x}) — inspecting UIA ===");
            for attempt in 0..12 {
                let Ok(element) = automation.ElementFromHandle(hwnd) else {
                    thread::sleep(Duration::from_millis(200));
                    continue;
                };
                let Ok(cond) = automation.CreateTrueCondition() else { break };
                let Ok(all): Result<windows::Win32::UI::Accessibility::IUIAutomationElementArray, _> =
                    element.FindAll(TreeScope_Descendants, &cond)
                else {
                    thread::sleep(Duration::from_millis(200));
                    continue;
                };
                let len = all.Length().unwrap_or(0);
                let mut buttons: Vec<String> = Vec::new();
                let mut type_hist: HashMap<i32, u32> = HashMap::new();
                for i in 0..len {
                    if let Ok(e) = all.GetElement(i) {
                        let ct = e.CurrentControlType().map(|c| c.0).unwrap_or(0);
                        *type_hist.entry(ct).or_insert(0) += 1;
                        if ct == UIA_ButtonControlTypeId.0 {
                            if let Ok(n) = e.CurrentName() {
                                let s = n.to_string();
                                if !s.trim().is_empty() {
                                    buttons.push(s);
                                }
                            }
                        }
                    }
                }
                if buttons.is_empty() && attempt < 11 {
                    thread::sleep(Duration::from_millis(200));
                    continue;
                }
                println!("elements={len}  control-type histogram (id:count): {type_hist:?}");
                println!("BUTTON labels: {buttons:?}");
                let low: Vec<String> = buttons.iter().map(|b| b.to_lowercase()).collect();
                let decline = low.iter().any(|n| ["decline", "ablehnen", "reject"].iter().any(|d| n.contains(d)));
                let accept = low.iter().any(|n| ["accept", "annehmen", "akzeptieren"].iter().any(|a| n.contains(a)));
                println!("VERDICT: accept_btn={accept} decline_btn={decline} => call_detected={}", accept && decline);
                break;
            }
        }
    });

    unsafe {
        let hook = SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            None,
            Some(cb),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        if hook.is_invalid() {
            eprintln!("SetWinEventHook failed");
            std::process::exit(1);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("window_probe only works on Windows.");
    std::process::exit(1);
}
