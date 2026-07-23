//! Fallback `CallSource`: tails new Teams' local log file for call-start
//! markers instead of reading any notification API. This is the same
//! technique classic-Teams tools used (e.g. `mre/teams-call` grepping
//! `eventData: s::;m::1;a::1` from `logs.txt`) adapted for new Teams.
//!
//! `CALL_START_MARKERS` is calibrated against real log lines (grepped
//! directly on a Windows box with real call history in
//! `%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\MSTeams\Logs\MSTeams_*.log`),
//! not guessed:
//! ```text
//! <INFO> HfpVoipCallCoordinatorImpl: reportIncomingCall for callId: <uuid>
//! <INFO> HfpVoipCallCoordinatorImpl: reportIncomingCall completed  for callId: <uuid>
//! ```
//! No caller name appears on or near this line (nearby lines are toast
//! window plumbing — see `window_win`'s doc comment for that data), so
//! `parse_call_start_line` always falls back to `"Unknown caller"`.
//!
//! Explicitly a last-resort fallback: log schemas are undocumented and
//! change across Teams releases with no deprecation notice, so this needs
//! re-calibration whenever it goes quiet. Prefer `window_win::WindowCallSource`
//! (M1) where it works.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use super::classify::IncomingCall;
use super::source::CallSource;

/// Substrings that, if found in a new-Teams log line, indicate an incoming
/// call is ringing. Calibrated against real log history (see module doc) —
/// `reportIncomingCall` (without "completed") is the ring; the "completed"
/// variant fires ~10ms later once Teams finishes registering it and would
/// double-count if also matched, so only the un-completed form is listed.
const CALL_START_MARKERS: &[&str] = &["HfpVoipCallCoordinatorImpl: reportIncomingCall for callId"];

/// Default new-Teams log directory relative to `%LOCALAPPDATA%`. New Teams
/// is an MSIX package, so logs live under `Packages\<PackageFamilyName>\...`
/// rather than the classic `%APPDATA%\Microsoft\Teams` location.
const DEFAULT_LOG_DIR_SUFFIX: &str =
    r"Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\MSTeams\Logs";

pub fn default_log_dir() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    Some(Path::new(&local_app_data).join(DEFAULT_LOG_DIR_SUFFIX))
}

/// Pure line parser — unit-testable without touching the filesystem. Returns
/// a caller name if the line looks like a call-start event; new Teams' logs
/// are not known to carry a caller name inline, so this currently always
/// falls back to `"Unknown caller"` pending calibration.
pub fn parse_call_start_line(line: &str) -> Option<IncomingCall> {
    let is_call_start = CALL_START_MARKERS.iter().any(|m| line.contains(m));
    if !is_call_start {
        return None;
    }
    Some(IncomingCall {
        caller: "Unknown caller".to_string(),
    })
}

/// The call-start marker only appears in the main renderer log, whose files
/// are named `MSTeams_<timestamp>.NN.log`. The directory also holds
/// `MSTeamsBackgroundEcs_*.log` and `MSTeamsUpdate_*.log`, which are written
/// on their own cadence and are frequently *more* recently modified than the
/// main log while idle — so a plain most-recent-`*.log` pick would tail the
/// wrong file and miss the ring. Requiring the `MSTeams_` prefix (note the
/// underscore: `MSTeamsBackground`/`MSTeamsUpdate` have letters before theirs)
/// selects only the main log.
fn is_main_log(name: &str) -> bool {
    name.starts_with("MSTeams_") && name.ends_with(".log")
}

/// Finds the most-recently-modified main renderer log under `dir` — new Teams
/// rotates these, so "the log" is whichever `MSTeams_*.log` is currently
/// being written to.
fn find_active_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| is_main_log(&e.file_name().to_string_lossy()))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

/// Generic new-line tailer over the active `MSTeams_*.log`, shared by
/// `LogTailCallSource` (ring detection) and `unread::UnreadCountSource`
/// (unread badge count) — same file, same rotation handling, different
/// per-line parsing. `on_line` returns `false` to stop the tailer early
/// (e.g. once its output channel's receiver has been dropped).
pub(crate) fn spawn_tailer(
    log_dir: PathBuf,
    poll: Duration,
    mut on_line: impl FnMut(&str) -> bool + Send + 'static,
) {
    thread::spawn(move || {
        let mut current_path: Option<PathBuf> = None;
        let mut offset: u64 = 0;

        loop {
            let active = find_active_log(&log_dir);
            match active {
                Some(path) => {
                    if current_path.as_deref() != Some(path.as_path()) {
                        if current_path.is_none() {
                            // First attach at startup: seek to end so we
                            // don't replay a whole prior session's history.
                            offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        } else {
                            // Mid-run rotation: Teams closed the old log
                            // and opened a fresh one. Read the new file
                            // from the start (offset 0) — it begins empty
                            // and grows from here, so byte 0 is the start
                            // of new content, and an event that's the very
                            // first line written after rotation is caught
                            // rather than skipped by seeking to end.
                            offset = 0;
                        }
                        current_path = Some(path);
                    }

                    if let Some(path) = &current_path {
                        if let Ok(mut file) = std::fs::File::open(path) {
                            if file.seek(SeekFrom::Start(offset)).is_ok() {
                                let mut buf = String::new();
                                if let Ok(n) = file.read_to_string(&mut buf) {
                                    if n > 0 {
                                        offset += n as u64;
                                        for line in buf.lines() {
                                            if !on_line(line) {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                None => {
                    tracing::warn!(dir = %log_dir.display(), "no Teams log file found");
                }
            }

            thread::sleep(poll);
        }
    });
}

pub struct LogTailCallSource {
    log_dir: PathBuf,
    poll: Duration,
}

impl LogTailCallSource {
    pub fn new(log_dir: PathBuf) -> Self {
        LogTailCallSource {
            log_dir,
            poll: Duration::from_secs(2),
        }
    }
}

impl CallSource for LogTailCallSource {
    fn start(self) -> Receiver<IncomingCall> {
        let (tx, rx) = mpsc::channel();
        spawn_tailer(self.log_dir, self.poll, move |line| {
            if let Some(call) = parse_call_start_line(line) {
                if tx.send(call).is_err() {
                    return false;
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

    #[test]
    fn recognizes_call_start_marker() {
        // Real new-Teams log line shape (see module doc).
        let line = "2026-07-06T09:51:23.289019+02:00 0x00005d98 <INFO> \
                    HfpVoipCallCoordinatorImpl: reportIncomingCall for callId: \
                    0abb5abf-bb00-484a-8966-9c7b27b2d027";
        assert_eq!(
            parse_call_start_line(line),
            Some(IncomingCall {
                caller: "Unknown caller".to_string()
            })
        );
    }

    #[test]
    fn ignores_call_completed_line_to_avoid_double_count() {
        // Fires ~10ms after the ring line; must NOT match or every call
        // would be reported twice.
        let line = "2026-07-06T09:51:23.298522+02:00 0x00005d98 <INFO> \
                    HfpVoipCallCoordinatorImpl: reportIncomingCall completed  for callId: \
                    0abb5abf-bb00-484a-8966-9c7b27b2d027";
        assert_eq!(parse_call_start_line(line), None);
    }

    #[test]
    fn ignores_unrelated_lines() {
        let line = "2026-07-07T10:00:00 [INFO] user presence changed to Available";
        assert_eq!(parse_call_start_line(line), None);
    }

    #[test]
    fn find_active_log_picks_most_recently_modified() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("MSTeams_2026-07-06_09-36-32.00.log");
        std::fs::write(&old, "old").unwrap();
        thread::sleep(Duration::from_millis(20));
        let newer = dir.path().join("MSTeams_2026-07-07_07-49-14.00.log");
        std::fs::write(&newer, "newer").unwrap();
        // A more-recently-modified non-main log must NOT win.
        let bg = dir.path().join("MSTeamsBackgroundEcs_2026-07-08_10-18-33.79.log");
        std::fs::write(&bg, "bg").unwrap();

        assert_eq!(find_active_log(dir.path()), Some(newer));
    }

    #[test]
    fn tails_only_newly_appended_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("MSTeams_2026-07-08_10-00-00.00.log");
        let marker = "HfpVoipCallCoordinatorImpl: reportIncomingCall for callId: x";
        std::fs::write(&log_path, format!("old line {marker}\n")).unwrap();

        let source = LogTailCallSource {
            log_dir: dir.path().to_path_buf(),
            poll: Duration::from_millis(20),
        };
        let rx = source.start();

        // Old content (already in the file before we started tailing) must
        // not be replayed.
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap();
        writeln!(file, "new line {marker}").unwrap();

        let call = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(call.caller, "Unknown caller");
    }

    #[test]
    fn call_on_freshly_rotated_log_is_not_missed() {
        // Regression: on mid-run rotation Teams opens a new empty log; a call
        // that's the FIRST line written to it must be caught, not skipped by
        // seeking to end-of-file on the switch.
        let dir = tempfile::tempdir().unwrap();
        let marker = "HfpVoipCallCoordinatorImpl: reportIncomingCall for callId: y";

        // Startup: one existing log with only history — must be skipped.
        let first = dir.path().join("MSTeams_2026-07-08_09-00-00.00.log");
        std::fs::write(&first, "boot line, not a call\n").unwrap();

        let source = LogTailCallSource {
            log_dir: dir.path().to_path_buf(),
            poll: Duration::from_millis(20),
        };
        let rx = source.start();
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

        // Rotation: a newer main log appears whose very first line is a call.
        thread::sleep(Duration::from_millis(30));
        let rotated = dir.path().join("MSTeams_2026-07-08_10-00-00.01.log");
        std::fs::write(&rotated, format!("{marker}\n")).unwrap();

        let call = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(call.caller, "Unknown caller");
    }
}
