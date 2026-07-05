use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Urgent,
    Deadline,
    Admin,
    Creative,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Urgent => "urgent",
            Category::Deadline => "deadline",
            Category::Admin => "admin",
            Category::Creative => "creative",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        match s {
            "urgent" => Some(Category::Urgent),
            "deadline" => Some(Category::Deadline),
            "admin" => Some(Category::Admin),
            "creative" => Some(Category::Creative),
            _ => None,
        }
    }

    pub const ALL: [Category; 4] = [
        Category::Urgent,
        Category::Deadline,
        Category::Admin,
        Category::Creative,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Untouched,
    Wip,
    Done,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Untouched => "untouched",
            Phase::Wip => "wip",
            Phase::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        match s {
            "untouched" => Some(Phase::Untouched),
            "wip" => Some(Phase::Wip),
            "done" => Some(Phase::Done),
            _ => None,
        }
    }

    pub fn next(self) -> Phase {
        match self {
            Phase::Untouched => Phase::Wip,
            Phase::Wip => Phase::Done,
            Phase::Done => Phase::Untouched,
        }
    }
}

/// Raw DB row shape — category/phase kept as text here (validated by CHECK
/// constraints), converted to the typed enums in `Task` for the API layer.
#[derive(Debug, sqlx::FromRow)]
pub struct TaskRow {
    pub id: i64,
    pub text: String,
    pub category: String,
    pub phase: String,
    pub assigned_date: Option<String>,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: i64,
    pub text: String,
    pub category: Category,
    pub phase: Phase,
    pub assigned_date: Option<String>,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

impl From<TaskRow> for Task {
    fn from(r: TaskRow) -> Self {
        Task {
            id: r.id,
            text: r.text,
            category: Category::parse(&r.category).expect("category CHECK constraint"),
            phase: Phase::parse(&r.phase).expect("phase CHECK constraint"),
            assigned_date: r.assigned_date,
            position: r.position,
            created_at: r.created_at,
            updated_at: r.updated_at,
            completed_at: r.completed_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateTask {
    pub text: String,
    pub category: Category,
    pub assigned_date: Option<String>,
    pub phase: Option<Phase>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PatchTask {
    pub text: Option<String>,
    pub category: Option<Category>,
    pub phase: Option<Phase>,
    #[serde(default, deserialize_with = "double_option")]
    pub assigned_date: Option<Option<String>>,
    pub position: Option<i64>,
}

/// Distinguishes "field absent" (None) from "field present but null" (Some(None)),
/// needed so PATCH can explicitly clear `assigned_date` back to backlog.
fn double_option<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct HistoryEntry {
    pub action: String,
    pub field: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub changed_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct CalendarEvent {
    pub id: i64,
    pub external_id: String,
    pub title: String,
    pub start_at: String,
    pub end_at: String,
    pub place: Option<String>,
    pub is_cancelled: bool,
    pub received_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CalendarEventIn {
    pub external_id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub place: Option<String>,
    #[serde(default)]
    pub is_cancelled: bool,
}

#[derive(Debug, Deserialize)]
pub struct CalendarPutBody {
    pub events: Vec<CalendarEventIn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamsKind {
    Call,
    Reminder,
    Info,
}

impl TeamsKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamsKind::Call => "call",
            TeamsKind::Reminder => "reminder",
            TeamsKind::Info => "info",
        }
    }

    pub fn parse(s: &str) -> Option<TeamsKind> {
        match s {
            "call" => Some(TeamsKind::Call),
            "reminder" => Some(TeamsKind::Reminder),
            "info" => Some(TeamsKind::Info),
            _ => None,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct TeamsEventRow {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub payload: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamsEvent {
    pub id: i64,
    pub kind: TeamsKind,
    pub text: String,
    pub payload: Option<serde_json::Value>,
    pub created_at: String,
}

impl From<TeamsEventRow> for TeamsEvent {
    fn from(r: TeamsEventRow) -> Self {
        TeamsEvent {
            id: r.id,
            kind: TeamsKind::parse(&r.kind).expect("kind CHECK constraint"),
            text: r.text,
            payload: r.payload.and_then(|p| serde_json::from_str(&p).ok()),
            created_at: r.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TeamsEventIn {
    pub kind: TeamsKind,
    pub text: String,
    pub payload: Option<serde_json::Value>,
}
