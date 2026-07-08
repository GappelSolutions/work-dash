//! Calibration tool: captures *every* Windows toast notification (any app)
//! for a fixed duration and appends each one to a log file. Run on the real
//! Windows machine to see what an actual Teams incoming-call toast looks
//! like (AUMID, text lines) before trusting `teams::classify`'s phrase list.
//!
//! Capture is event-driven via `NotificationChanged` — this fires the
//! moment a toast is added, which matters because Teams marks its toasts
//! transient (banner shows, then the toast is removed without ever landing
//! in the Notification Center). A poll-only approach misses those. A slow
//! poll sweep is kept as fallback for anything the event misses.
//!
//! Env vars:
//!   TOAST_LOG_PATH          default "toast-log.txt"
//!   TOAST_LOG_DURATION_SECS default 3600 (1 hour)

#[cfg(windows)]
fn main() {
    use std::collections::HashSet;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::KnownNotificationBindings;
    use windows::UI::Notifications::Management::{
        UserNotificationListener, UserNotificationListenerAccessStatus,
    };
    use windows::UI::Notifications::{NotificationKinds, UserNotification};

    // Env vars first, then positional CLI args (log_path, duration_secs) —
    // `Invoke-CommandInDesktopPackage` (needed for package identity) passes
    // command-line args through but does not inherit the caller's env, so
    // args are the only reliable path once launched that way.
    let args: Vec<String> = std::env::args().collect();
    let log_path = std::env::var("TOAST_LOG_PATH")
        .ok()
        .or_else(|| args.get(1).cloned())
        .unwrap_or_else(|| "toast-log.txt".to_string());
    let duration_secs: u64 = std::env::var("TOAST_LOG_DURATION_SECS")
        .ok()
        .or_else(|| args.get(2).cloned())
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600);

    println!("logging all toast notifications to {log_path} for {duration_secs}s");

    // Package identity diagnostic: NotificationChanged silently requires it
    // (0x80070490 without), so make its presence visible before subscribing.
    let identity_line = {
        use windows::core::PWSTR;
        use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
        let mut len = 0u32;
        let probe = unsafe { GetCurrentPackageFullName(&mut len, None) };
        const APPMODEL_ERROR_NO_PACKAGE: u32 = 15700;
        if probe.0 == APPMODEL_ERROR_NO_PACKAGE {
            "package identity: NONE (running unpackaged)".to_string()
        } else {
            let mut buf = vec![0u16; len as usize];
            let rc = unsafe { GetCurrentPackageFullName(&mut len, Some(PWSTR(buf.as_mut_ptr()))) };
            if rc.0 == 0 {
                let name = String::from_utf16_lossy(&buf[..len.saturating_sub(1) as usize]);
                format!("package identity: {name}")
            } else {
                format!("package identity: probe failed rc={}", rc.0)
            }
        }
    };
    println!("{identity_line}");

    let listener = UserNotificationListener::Current().expect("get UserNotificationListener");
    let access = listener
        .RequestAccessAsync()
        .and_then(|op| op.join())
        .expect("request notification access");

    if access != UserNotificationListenerAccessStatus::Allowed {
        eprintln!(
            "notification access not granted ({access:?}) — allow it in \
             Settings > Privacy > Notifications and re-run"
        );
        std::process::exit(1);
    }

    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("open log file");

    // The process may be spawned detached (Invoke-CommandInDesktopPackage),
    // so stdout can vanish — persist run diagnostics in the log itself.
    let header = format!(
        "=== run start {} | {identity_line} | duration {duration_secs}s\n",
        chrono::Utc::now().to_rfc3339()
    );
    log_file.write_all(header.as_bytes()).expect("write header");

    fn describe(notification: &UserNotification) -> String {
        let id = notification.Id().unwrap_or(0);

        let aumid = notification
            .AppInfo()
            .and_then(|info| info.AppUserModelId())
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|_| "<unknown-aumid>".to_string());

        let display_name = notification
            .AppInfo()
            .and_then(|info| info.DisplayInfo())
            .and_then(|d| d.DisplayName())
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();

        let lines: Vec<String> = (|| -> windows::core::Result<Vec<String>> {
            let toast = notification.Notification()?;
            let visual = toast.Visual()?;
            let binding_id = KnownNotificationBindings::ToastGeneric()?;
            let binding = visual.GetBinding(&binding_id)?;
            let elements = binding.GetTextElements()?;
            Ok(elements
                .into_iter()
                .filter_map(|e| e.Text().ok())
                .map(|s| s.to_string_lossy())
                .collect())
        })()
        .unwrap_or_default();

        let timestamp = chrono::Utc::now().to_rfc3339();
        format!("{timestamp} id={id} aumid={aumid} display={display_name:?} lines={lines:?}\n")
    }

    // Event handler fires on every add/remove. It only forwards the id —
    // the main thread does the (fallible) lookup and logging. Grabbing the
    // notification immediately in the handler is the whole point: transient
    // toasts are gone milliseconds later.
    let (tx, rx) = mpsc::channel::<String>();
    let handler_listener = listener.clone();
    let handler_tx = tx.clone();
    let subscribe_result = listener
        .NotificationChanged(&TypedEventHandler::new(move |_, args: windows::core::Ref<
            windows::UI::Notifications::UserNotificationChangedEventArgs,
        >| {
            if let Some(args) = args.as_ref() {
                if let Ok(id) = args.UserNotificationId() {
                    match handler_listener.GetNotification(id) {
                        Ok(notification) => {
                            let _ = handler_tx.send(describe(&notification));
                        }
                        Err(_) => {
                            // Removed (or already gone) — still worth a line so
                            // we can see transient toasts disappearing.
                            let timestamp = chrono::Utc::now().to_rfc3339();
                            let _ = handler_tx
                                .send(format!("{timestamp} id={id} <removed or not readable>\n"));
                        }
                    }
                }
            }
            Ok(())
        }));
    let subscribe_line = match &subscribe_result {
        Ok(_) => "NotificationChanged subscription: OK (event-driven capture active)".to_string(),
        Err(e) => format!(
            "NotificationChanged subscription FAILED ({e:?}) — falling back to 250ms polling only. \
             Transient toasts (Teams) may still be missed."
        ),
    };
    println!("{subscribe_line}");
    log_file
        .write_all(format!("{subscribe_line}\n").as_bytes())
        .expect("write subscribe status");
    let token = subscribe_result.ok();

    let mut seen: HashSet<String> = HashSet::new();
    let deadline = Instant::now() + Duration::from_secs(duration_secs);

    let mut write_entry = |entry: String, log_file: &mut std::fs::File| {
        // Dedup on the id+content portion (skip the timestamp prefix).
        let key = entry.splitn(2, " id=").nth(1).unwrap_or(&entry).to_string();
        if seen.insert(key) {
            print!("{entry}");
            log_file.write_all(entry.as_bytes()).expect("write log entry");
            log_file.flush().expect("flush log");
        }
    };

    while Instant::now() < deadline {
        // Drain event-driven captures.
        while let Ok(entry) = rx.try_recv() {
            write_entry(entry, &mut log_file);
        }

        // Fallback sweep for anything the event missed.
        if let Ok(notifications) = listener
            .GetNotificationsAsync(NotificationKinds::Toast)
            .and_then(|op| op.join())
        {
            for notification in &notifications {
                write_entry(describe(&notification), &mut log_file);
            }
        }

        std::thread::sleep(Duration::from_millis(250));
    }

    if let Some(token) = token {
        let _ = listener.RemoveNotificationChanged(token);
    }
    println!("done — log written to {log_path}");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("toast_logger only works on Windows (needs UserNotificationListener).");
    std::process::exit(1);
}
