//! Calibration tool for `teams::logtail`: finds new Teams' local log
//! directory, tails the most recently modified log file, and echoes every
//! new line with a timestamp prefix. Run during a real test call/chat and
//! grep the output for a line that reliably marks call-start — feed that
//! marker into `logtail::CALL_START_MARKERS`.
//!
//! Env vars:
//!   TEAMS_LOG_DIR override (defaults to `logtail::default_log_dir()`)

use work_dash_windows_client::teams::logtail::default_log_dir;

fn main() {
    let dir = std::env::var("TEAMS_LOG_DIR")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(default_log_dir);

    let Some(dir) = dir else {
        eprintln!(
            "could not resolve a Teams log directory — set TEAMS_LOG_DIR explicitly \
             (LOCALAPPDATA not set, or not running on Windows)"
        );
        std::process::exit(1);
    };

    println!(
        "watching {} for new Teams log lines (Ctrl+C to stop)",
        dir.display()
    );
    if !dir.exists() {
        println!(
            "warning: directory does not exist yet — new Teams may use a different path; \
             check %LOCALAPPDATA%\\Packages\\ for the actual MSTeams package folder name"
        );
    }

    let mut current_path: Option<std::path::PathBuf> = None;
    let mut offset: u64 = 0;

    loop {
        // Match the source's filter (`teams::logtail::is_main_log`): only the
        // main `MSTeams_*.log` carries the call marker. The dir is dominated
        // by `MSTeamsBackgroundEcs_*.log` heartbeats that are frequently more
        // recent, so a broad filter would tail the wrong file.
        let active = std::fs::read_dir(&dir).ok().and_then(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with("MSTeams_") && name.ends_with(".log")
                })
                .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
                .map(|e| e.path())
        });

        if let Some(path) = active {
            if current_path.as_deref() != Some(path.as_path()) {
                println!("--- switched to log file: {} ---", path.display());
                offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                current_path = Some(path);
            }

            if let Some(path) = &current_path {
                use std::io::{Read, Seek, SeekFrom};
                if let Ok(mut file) = std::fs::File::open(path) {
                    if file.seek(SeekFrom::Start(offset)).is_ok() {
                        let mut buf = String::new();
                        if let Ok(n) = file.read_to_string(&mut buf) {
                            if n > 0 {
                                offset += n as u64;
                                for line in buf.lines() {
                                    println!("[{}] {line}", chrono::Utc::now().to_rfc3339());
                                }
                            }
                        }
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
