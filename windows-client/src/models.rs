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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamsKind {
    Call,
    Reminder,
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TeamsEventIn {
    pub kind: TeamsKind,
    pub text: String,
    pub payload: Option<serde_json::Value>,
}
