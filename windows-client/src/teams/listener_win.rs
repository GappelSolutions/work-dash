//! LEGACY / KNOWN NON-FUNCTIONAL for current new-Teams builds: watches the
//! Windows `UserNotificationListener` for Teams "incoming call" toasts.
//! Calibration (`toast-log.txt`, a 1h capture with `NotificationChanged`
//! event-driven listening and confirmed package identity) caught Outlook
//! and system toasts but zero Teams notifications — new Teams does not
//! route its ring/chat popups through the real Windows toast pipeline, so
//! this API structurally cannot see them. Kept only as an explicit opt-in
//! (`TEAMS_CALL_SOURCE=toast`) in case a future Teams build changes this;
//! prefer `window_win::WindowCallSource` or `logtail::LogTailCallSource`.
//! Windows-only — requires the process to have package identity
//! (sparse/external-location package). Not exercised by `cargo test` on
//! non-Windows hosts; `listener_mock` stands in for it there.

use std::sync::mpsc::{self, Receiver};
use std::thread;

use windows::UI::Notifications::KnownNotificationBindings;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::NotificationKinds;

use super::classify::{CallClassifier, IncomingCall};
use super::source::CallSource;

pub struct ToastCallSource {
    classifier: CallClassifier,
}

impl ToastCallSource {
    pub fn new(classifier: CallClassifier) -> Self {
        ToastCallSource { classifier }
    }

    fn poll_once(&self, listener: &UserNotificationListener) -> windows::core::Result<Vec<IncomingCall>> {
        let notifications = listener
            .GetNotificationsAsync(NotificationKinds::Toast)?
            .join()?;

        let mut calls = Vec::new();
        for notification in &notifications {
            let app_info = notification.AppInfo()?;
            let aumid = app_info.AppUserModelId()?.to_string_lossy();

            let toast = notification.Notification()?;
            let visual = toast.Visual()?;
            let binding = visual.GetBinding(&KnownNotificationBindings::ToastGeneric()?)?;
            let text_elements = binding.GetTextElements()?;

            let lines: Vec<String> = text_elements
                .into_iter()
                .filter_map(|e| e.Text().ok())
                .map(|s| s.to_string_lossy())
                .collect();

            if let Some(call) = self.classifier.classify(&aumid, &lines) {
                calls.push(call);
            }
        }
        Ok(calls)
    }
}

impl CallSource for ToastCallSource {
    fn start(self) -> Receiver<IncomingCall> {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let listener = match UserNotificationListener::Current() {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!(?e, "failed to get UserNotificationListener");
                    return;
                }
            };

            let access = match listener.RequestAccessAsync().and_then(|op| op.join()) {
                Ok(status) => status,
                Err(e) => {
                    tracing::error!(?e, "notification access request failed");
                    return;
                }
            };

            if access != UserNotificationListenerAccessStatus::Allowed {
                tracing::error!(?access, "user notification access not granted");
                return;
            }

            // Event-driven via NotificationChanged would be lower-latency;
            // a short poll loop is simpler and robust to missed events.
            loop {
                match self.poll_once(&listener) {
                    Ok(calls) => {
                        for call in calls {
                            if tx.send(call).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) => tracing::warn!(?e, "toast poll failed"),
                }
                thread::sleep(std::time::Duration::from_secs(2));
            }
        });

        rx
    }
}
