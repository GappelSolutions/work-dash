//! Offline fallback data — used only when `WORK_DASH_SERVER_URL` /
//! `WORK_DASH_API_KEY` aren't set (see `net.rs`), so the client still runs
//! standalone.

use chrono::{Duration, Local};

use crate::app::{CalendarEvent, Card, Column, Phase};

pub fn calendar_events() -> Vec<CalendarEvent> {
    let today = Local::now().date_naive();
    let at = |h: u32, m: u32| {
        today
            .and_hms_opt(h, m, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
    };
    let ev = |sh, sm, eh, em, title: &str, place: Option<&str>| CalendarEvent {
        start: at(sh, sm),
        end: at(eh, em),
        title: title.to_string(),
        place: place.map(str::to_string),
    };
    vec![
        ev(9, 15, 9, 30, "Daily standup", Some("Teams")),
        ev(11, 0, 12, 0, "Sprint review", Some("Teams")),
        ev(14, 30, 15, 0, "1:1 with lead", Some("Teams")),
        ev(15, 30, 17, 0, "Focus block: dashboard client", None),
    ]
}

pub fn unread_count() -> u32 {
    3
}

/// Hourly break alarm — hardcoded interval for now (see ARCHITECTURE.md).
pub fn next_break() -> chrono::DateTime<Local> {
    Local::now() + Duration::hours(1)
}

pub fn kanban() -> Vec<Column> {
    let col = |title: &str, cards: &[(&str, Phase)]| Column {
        title: title.to_string(),
        cards: cards
            .iter()
            .map(|(text, phase)| Card {
                text: text.to_string(),
                phase: *phase,
                id: None,
            })
            .collect(),
    };
    vec![
        col(
            "URGENT",
            &[
                ("Prod alert triage", Phase::Wip),
                ("Reply to on-call ping", Phase::Untouched),
                ("Restart WS listener on Pi", Phase::Done),
            ],
        ),
        col(
            "DEADLINE",
            &[
                ("Sprint review deck", Phase::Untouched),
                ("Client demo prep", Phase::Wip),
                ("Quarterly report draft", Phase::Untouched),
            ],
        ),
        col(
            "ADMIN",
            &[
                ("Submit timesheet", Phase::Untouched),
                ("Expense report", Phase::Done),
                ("Inbox zero", Phase::Wip),
            ],
        ),
        col(
            "CREATIVE",
            &[
                ("Aquarium palette experiment", Phase::Wip),
                ("Video-to-ASCII shader sketch", Phase::Untouched),
                ("Dashboard font exploration", Phase::Done),
            ],
        ),
    ]
}
