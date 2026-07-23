//! Wire types matching `work-dash-server`'s ingest contract. No shared crate
//! exists between server and clients (see `server/src/models.rs`,
//! `client/src/net.rs`) — these are the windows-client's copy, kept in sync
//! by hand.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalendarEventIn {
    pub external_id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub place: Option<String>,
    #[serde(rename = "is_cancelled")]
    pub is_cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalendarPutBody {
    pub events: Vec<CalendarEventIn>,
    pub range_start: String,
    pub range_end: String,
}

/// Body for `PUT /api/teams` — the absolute unread count from this poll,
/// not a delta (see `graph::chats`'s poll-and-diff loop).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SetUnreadCount {
    pub count: i64,
}

/// Body for `PUT /api/call` — caller name only; the server holds this as an
/// ephemeral singleton, not a log entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PutCallBody {
    pub caller: String,
}
