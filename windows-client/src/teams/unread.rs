//! Local, Graph-free unread-chat-count tracking. Tails the same new-Teams
//! log file as `logtail`'s call detector (via the shared `spawn_tailer`),
//! watching for a different marker: `unread notification count: <n>`, which
//! Teams itself writes whenever its own unread badge changes.
//!
//! Confirmed against a full day of real capture (`teams_day_capture.ps1`'s
//! `UNREAD` tag, `teams-events.txt`) — fires reliably and carries only the
//! bare integer, never message content or sender. This is the local
//! equivalent of how `outlook_calendar_push.ps1` reads Outlook via COM
//! automation instead of Graph: no Microsoft sign-in, no cloud token, just
//! the already-running desktop app's own local state. Teams reports the
//! absolute badge count each time, so pushing it straight through (see
//! `main.rs::run_unread_loop`) is poll-and-diff in spirit without actually
//! polling anything — self-corrects on every change, no local read/unread
//! state kept here.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::logtail::spawn_tailer;

const UNREAD_MARKER: &str = "unread notification count: ";

/// Pure line parser — unit-testable without touching the filesystem.
pub fn parse_unread_count_line(line: &str) -> Option<i64> {
    let idx = line.find(UNREAD_MARKER)?;
    let rest = &line[idx + UNREAD_MARKER.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

pub struct UnreadCountSource {
    log_dir: PathBuf,
    poll: Duration,
}

impl UnreadCountSource {
    pub fn new(log_dir: PathBuf) -> Self {
        UnreadCountSource {
            log_dir,
            poll: Duration::from_secs(2),
        }
    }

    /// Starts tailing in the background; yields the new absolute count each
    /// time it changes (deduped here, mirroring `teams_day_capture.ps1`'s
    /// `$lastUnread` guard, so an unchanged count read repeatedly from the
    /// log doesn't spam redundant pushes).
    pub fn start(self) -> Receiver<i64> {
        let (tx, rx) = mpsc::channel();
        let mut last: Option<i64> = None;
        spawn_tailer(self.log_dir, self.poll, move |line| {
            if let Some(n) = parse_unread_count_line(line) {
                if last != Some(n) {
                    last = Some(n);
                    if tx.send(n).is_err() {
                        return false;
                    }
                }
            }
            true
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn parses_unread_count_from_real_shaped_line() {
        let line = "2026-07-09T10:43:04.0229653+02:00 0x00003a10 <INFO> \
                    NotificationHandler: unread notification count: 1 changed";
        assert_eq!(parse_unread_count_line(line), Some(1));
    }

    #[test]
    fn parses_zero() {
        let line = "unread notification count: 0";
        assert_eq!(parse_unread_count_line(line), Some(0));
    }

    #[test]
    fn ignores_unrelated_lines() {
        let line = "2026-07-09T10:43:04+02:00 <INFO> user presence changed to Available";
        assert_eq!(parse_unread_count_line(line), None);
    }

    #[test]
    fn tails_only_newly_appended_lines_and_dedupes_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("MSTeams_2026-07-08_10-00-00.00.log");
        std::fs::write(&log_path, "old unread notification count: 5\n").unwrap();

        let source = UnreadCountSource {
            log_dir: dir.path().to_path_buf(),
            poll: Duration::from_millis(20),
        };
        let rx = source.start();

        // Pre-existing content must not be replayed.
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap();
        writeln!(file, "unread notification count: 2").unwrap();
        writeln!(file, "unread notification count: 2").unwrap();

        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
        // The repeat must be deduped, not sent again.
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());

        thread::sleep(Duration::from_millis(20));
        writeln!(file, "unread notification count: 0").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
    }
}
