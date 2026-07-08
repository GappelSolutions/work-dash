//! Pure translation from Microsoft Graph `calendarView` JSON to the
//! server's `CalendarEventIn` wire shape. No I/O, no time-of-day-dependent
//! behavior — fully unit-testable.

use serde::Deserialize;

use crate::models::CalendarEventIn;

#[derive(Debug, Deserialize)]
pub struct GraphDateTimeTimeZone {
    #[serde(rename = "dateTime")]
    pub date_time: String,
    #[allow(dead_code)]
    #[serde(rename = "timeZone")]
    pub time_zone: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct GraphLocation {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GraphOnlineMeeting {
    #[serde(rename = "joinUrl")]
    pub join_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GraphEvent {
    pub id: String,
    pub subject: Option<String>,
    #[serde(rename = "isCancelled", default)]
    pub is_cancelled: bool,
    pub start: GraphDateTimeTimeZone,
    pub end: GraphDateTimeTimeZone,
    #[serde(default)]
    pub location: Option<GraphLocation>,
    #[serde(rename = "onlineMeeting", default)]
    pub online_meeting: Option<GraphOnlineMeeting>,
}

/// Graph is queried with `Prefer: outlook.timezone="UTC"`, so `dateTime`
/// arrives as UTC wall-clock without an offset (e.g.
/// `"2026-07-06T09:15:00.0000000"`). Trim sub-second digits and append `Z`
/// to get a valid RFC3339 timestamp.
pub fn graph_datetime_to_rfc3339(date_time: &str) -> String {
    let base = date_time.split('.').next().unwrap_or(date_time);
    format!("{base}Z")
}

pub fn map_graph_event(ev: &GraphEvent) -> CalendarEventIn {
    let title = match ev.subject.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => "(no subject)".to_string(),
    };

    let place = ev
        .online_meeting
        .as_ref()
        .and_then(|m| m.join_url.clone())
        .or_else(|| {
            ev.location
                .as_ref()
                .and_then(|l| l.display_name.clone())
                .filter(|s| !s.trim().is_empty())
        });

    CalendarEventIn {
        external_id: ev.id.clone(),
        title,
        start: graph_datetime_to_rfc3339(&ev.start.date_time),
        end: graph_datetime_to_rfc3339(&ev.end.date_time),
        place,
        is_cancelled: ev.is_cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(json: serde_json::Value) -> GraphEvent {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn maps_basic_fields() {
        let ev = sample_event(serde_json::json!({
            "id": "abc123",
            "subject": "Standup",
            "isCancelled": false,
            "start": {"dateTime": "2026-07-06T09:15:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-07-06T09:30:00.0000000", "timeZone": "UTC"},
        }));
        let mapped = map_graph_event(&ev);
        assert_eq!(mapped.external_id, "abc123");
        assert_eq!(mapped.title, "Standup");
        assert_eq!(mapped.start, "2026-07-06T09:15:00Z");
        assert_eq!(mapped.end, "2026-07-06T09:30:00Z");
        assert_eq!(mapped.place, None);
        assert!(!mapped.is_cancelled);
    }

    #[test]
    fn empty_subject_becomes_placeholder() {
        let ev = sample_event(serde_json::json!({
            "id": "id1",
            "subject": "",
            "start": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
        }));
        assert_eq!(map_graph_event(&ev).title, "(no subject)");
    }

    #[test]
    fn missing_subject_becomes_placeholder() {
        let ev = sample_event(serde_json::json!({
            "id": "id1",
            "start": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
        }));
        assert_eq!(map_graph_event(&ev).title, "(no subject)");
    }

    #[test]
    fn online_meeting_join_url_preferred_over_location() {
        let ev = sample_event(serde_json::json!({
            "id": "id1",
            "subject": "Sync",
            "start": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "location": {"displayName": "Room 4"},
            "onlineMeeting": {"joinUrl": "https://teams.microsoft.com/l/meetup/xyz"},
        }));
        assert_eq!(
            map_graph_event(&ev).place,
            Some("https://teams.microsoft.com/l/meetup/xyz".to_string())
        );
    }

    #[test]
    fn falls_back_to_location_when_no_online_meeting() {
        let ev = sample_event(serde_json::json!({
            "id": "id1",
            "subject": "Sync",
            "start": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "location": {"displayName": "Room 4"},
        }));
        assert_eq!(map_graph_event(&ev).place, Some("Room 4".to_string()));
    }

    #[test]
    fn blank_location_display_name_yields_no_place() {
        let ev = sample_event(serde_json::json!({
            "id": "id1",
            "subject": "Sync",
            "start": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "location": {"displayName": "  "},
        }));
        assert_eq!(map_graph_event(&ev).place, None);
    }

    #[test]
    fn cancelled_event_is_flagged() {
        let ev = sample_event(serde_json::json!({
            "id": "id1",
            "subject": "Cancelled thing",
            "isCancelled": true,
            "start": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
        }));
        assert!(map_graph_event(&ev).is_cancelled);
    }

    #[test]
    fn datetime_without_fraction_still_gets_z_suffix() {
        assert_eq!(
            graph_datetime_to_rfc3339("2026-07-06T09:00:00"),
            "2026-07-06T09:00:00Z"
        );
    }
}
