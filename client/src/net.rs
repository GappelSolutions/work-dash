//! Background networking: talks to `work-dash-server` over REST + SSE.
//!
//! Only activates when both `WORK_DASH_SERVER_URL` and `WORK_DASH_API_KEY`
//! are set at startup — otherwise the app stays fully offline on seed data,
//! unchanged from before this module existed.

use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

const RECONNECT_DELAY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Deserialize)]
pub struct ServerTask {
    pub id: i64,
    pub text: String,
    pub category: String,
    pub phase: String,
    pub assigned_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerCalendarEvent {
    pub title: String,
    pub start_at: String,
    pub end_at: String,
    pub place: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnreadCount {
    pub count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerCallState {
    pub active: bool,
    pub caller: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub tasks: Vec<ServerTask>,
    pub calendar: Vec<ServerCalendarEvent>,
    pub unread_count: i64,
    pub call: ServerCallState,
}

/// Everything the network thread can hand back to the render loop.
#[derive(Debug, Clone)]
pub enum NetEvent {
    Connected,
    Disconnected,
    Snapshot(Snapshot),
    TaskUpserted(ServerTask),
    TaskDeleted { id: i64 },
    CalendarUpdated { events: Vec<ServerCalendarEvent> },
    UnreadCount(i64),
    CallState { active: bool, caller: Option<String> },
}

#[derive(Clone)]
pub struct NetConfig {
    pub server_url: String,
    pub api_key: String,
}

impl NetConfig {
    /// Reads `WORK_DASH_SERVER_URL` / `WORK_DASH_API_KEY` once at startup.
    /// Both must be set or networking stays off (seed-data fallback).
    pub fn from_env() -> Option<NetConfig> {
        let server_url = std::env::var("WORK_DASH_SERVER_URL").ok()?;
        let api_key = std::env::var("WORK_DASH_API_KEY").ok()?;
        Some(NetConfig {
            server_url: server_url.trim_end_matches('/').to_string(),
            api_key,
        })
    }
}

/// Spawns the background thread; returns immediately. The thread runs until
/// the process exits — reconnect/resync on any failure is handled internally.
pub fn spawn(config: NetConfig, tx: Sender<NetEvent>) {
    thread::spawn(move || loop {
        if run_once(&config, &tx).is_err() {
            let _ = tx.send(NetEvent::Disconnected);
        }
        thread::sleep(RECONNECT_DELAY);
    });
}

/// Fire a one-off PATCH (e.g. a phase cycle from a tap) without blocking the
/// render loop or queuing — call volume from a single kanban is low enough
/// that "spawn a thread per request" is simpler than a request queue.
pub fn patch_task_phase(config: &NetConfig, task_id: i64, phase: &str) {
    let config = config.clone();
    let phase = phase.to_string();
    thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        let _ = client
            .patch(format!("{}/api/tasks/{}", config.server_url, task_id))
            .bearer_auth(&config.api_key)
            .json(&serde_json::json!({ "phase": phase }))
            .send();
    });
}

/// Dismisses the call banner server-side — fire-and-forget, same rationale
/// as `patch_task_phase`. The SSE `call_state` event (or the next reconnect
/// snapshot) is what actually clears other listeners' view of it; this call
/// just tells the server "gone".
pub fn dismiss_call(config: &NetConfig) {
    let config = config.clone();
    thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        let _ = client
            .delete(format!("{}/api/call", config.server_url))
            .bearer_auth(&config.api_key)
            .send();
    });
}

fn run_once(config: &NetConfig, tx: &Sender<NetEvent>) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    let today = chrono::Local::now().date_naive();

    let tasks: Vec<ServerTask> = client
        .get(format!(
            "{}/api/tasks?scope=day&date={}",
            config.server_url, today
        ))
        .bearer_auth(&config.api_key)
        .send()?
        .error_for_status()?
        .json()?;

    let calendar: Vec<ServerCalendarEvent> = client
        .get(format!("{}/api/calendar", config.server_url))
        .bearer_auth(&config.api_key)
        .send()?
        .error_for_status()?
        .json()?;

    let unread: UnreadCount = client
        .get(format!("{}/api/teams", config.server_url))
        .bearer_auth(&config.api_key)
        .send()?
        .error_for_status()?
        .json()?;

    let call: ServerCallState = client
        .get(format!("{}/api/call", config.server_url))
        .bearer_auth(&config.api_key)
        .send()?
        .error_for_status()?
        .json()?;

    let _ = tx.send(NetEvent::Connected);
    let _ = tx.send(NetEvent::Snapshot(Snapshot {
        tasks,
        calendar,
        unread_count: unread.count,
        call,
    }));

    let resp = client
        .get(format!("{}/api/events", config.server_url))
        .bearer_auth(&config.api_key)
        .header("Accept", "text/event-stream")
        .send()?
        .error_for_status()?;

    let reader = BufReader::new(resp);
    let mut event_name = String::new();
    let mut data = String::new();

    // Any I/O error on the stream ends this connection attempt; the outer
    // loop in `spawn` reconnects and resyncs from scratch.
    for line in reader.lines() {
        let line = line?;

        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data = rest.trim().to_string();
        } else if line.is_empty() {
            dispatch_sse(&event_name, &data, tx);
            event_name.clear();
            data.clear();
        }
    }

    Ok(())
}

fn dispatch_sse(event_name: &str, data: &str, tx: &Sender<NetEvent>) {
    match event_name {
        "task_upserted" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(task) = v.get("task").and_then(|t| ServerTask::deserialize(t).ok()) {
                    let _ = tx.send(NetEvent::TaskUpserted(task));
                }
            }
        }
        "task_deleted" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(id) = v.get("id").and_then(|i| i.as_i64()) {
                    let _ = tx.send(NetEvent::TaskDeleted { id });
                }
            }
        }
        "calendar_updated" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(events) = v
                    .get("events")
                    .and_then(|e| serde_json::from_value::<Vec<ServerCalendarEvent>>(e.clone()).ok())
                {
                    let _ = tx.send(NetEvent::CalendarUpdated { events });
                }
            }
        }
        "unread_count" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(count) = v.get("count").and_then(|c| c.as_i64()) {
                    let _ = tx.send(NetEvent::UnreadCount(count));
                }
            }
        }
        "call_state" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(active) = v.get("active").and_then(|a| a.as_bool()) {
                    let caller = v
                        .get("caller")
                        .and_then(|c| c.as_str())
                        .map(str::to_string);
                    let _ = tx.send(NetEvent::CallState { active, caller });
                }
            }
        }
        // "hello" and "ping" carry no state the app needs to act on.
        _ => {}
    }
}
