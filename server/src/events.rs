use serde::Serialize;
use tokio::sync::broadcast;

use crate::models::{CalendarEvent, Task, TeamsEvent};

/// Fan-out channel: every mutating handler sends one of these after its DB
/// commit; `/api/events` subscribes and maps each into an SSE frame.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "task_upserted")]
    TaskUpserted { task: Task },
    #[serde(rename = "task_deleted")]
    TaskDeleted { id: i64 },
    #[serde(rename = "calendar_updated")]
    CalendarUpdated { date: String, events: Vec<CalendarEvent> },
    #[serde(rename = "teams_event")]
    TeamsEventFired(TeamsEvent),
}

impl ServerEvent {
    /// SSE `event:` line name — kept separate from the JSON `type` tag so
    /// clients can filter by SSE event type without parsing the payload.
    pub fn event_name(&self) -> &'static str {
        match self {
            ServerEvent::TaskUpserted { .. } => "task_upserted",
            ServerEvent::TaskDeleted { .. } => "task_deleted",
            ServerEvent::CalendarUpdated { .. } => "calendar_updated",
            ServerEvent::TeamsEventFired(_) => "teams_event",
        }
    }
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<ServerEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        // Bounded to avoid unbounded growth if a subscriber stalls; slow
        // subscribers just miss old events (they resync via GET on reconnect).
        let (tx, _rx) = broadcast::channel(256);
        EventBus { tx }
    }

    pub fn publish(&self, event: ServerEvent) {
        // No subscribers is a normal state (no client connected yet) — ignore.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broadcast_reaches_multiple_subscribers() {
        let bus = EventBus::new();
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();

        bus.publish(ServerEvent::TaskDeleted { id: 42 });

        let ea = a.recv().await.unwrap();
        let eb = b.recv().await.unwrap();
        assert_eq!(ea.event_name(), "task_deleted");
        assert_eq!(eb.event_name(), "task_deleted");
    }
}
