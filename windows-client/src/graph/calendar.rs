//! Fetches `/me/calendarView` (recurrence-expanded occurrences within a
//! window), paging through `@odata.nextLink`. Page parsing is pure and
//! unit-tested; `fetch_calendar_view` does the actual paged HTTP calls.

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::mapping::GraphEvent;

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

pub struct CalendarClient {
    client: reqwest::blocking::Client,
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct CalendarViewPage {
    value: Vec<GraphEvent>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

/// Parses one page of a `calendarView` response. Pure — no I/O.
pub fn parse_calendar_page(body: &str) -> Result<(Vec<GraphEvent>, Option<String>), String> {
    let page: CalendarViewPage = serde_json::from_str(body).map_err(|e| e.to_string())?;
    Ok((page.value, page.next_link))
}

/// Computes the `[startDateTime, endDateTime]` window for `calendarView`,
/// relative to `now`, in the UTC "Z" form matching the `Prefer:
/// outlook.timezone="UTC"` request header. `now` is a parameter (not
/// `Utc::now()`) so this stays pure and deterministic under test.
pub fn calendar_view_range(now: DateTime<Utc>, days_past: i64, days_future: i64) -> (String, String) {
    let start = now - Duration::days(days_past);
    let end = now + Duration::days(days_future);
    (
        start.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        end.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    )
}

impl CalendarClient {
    pub fn new(access_token: impl Into<String>) -> Self {
        CalendarClient {
            client: reqwest::blocking::Client::new(),
            access_token: access_token.into(),
        }
    }

    pub fn fetch_calendar_view(
        &self,
        start_date_time: &str,
        end_date_time: &str,
    ) -> Result<Vec<GraphEvent>, String> {
        let mut url = format!(
            "{GRAPH_BASE}/me/calendarView?startDateTime={start_date_time}&endDateTime={end_date_time}&$top=50"
        );
        let mut events = Vec::new();

        loop {
            let resp = self
                .client
                .get(&url)
                .bearer_auth(&self.access_token)
                .header("Prefer", r#"outlook.timezone="UTC""#)
                .send()
                .map_err(|e| e.to_string())?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();
                return Err(format!("calendarView request failed: {status} {body}"));
            }

            let body = resp.text().map_err(|e| e.to_string())?;
            let (mut page_events, next_link) = parse_calendar_page(&body)?;
            events.append(&mut page_events);

            match next_link {
                Some(link) => url = link,
                None => break,
            }
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parses_page_with_no_next_link() {
        let body = serde_json::json!({
            "value": [
                {
                    "id": "id1",
                    "subject": "Standup",
                    "start": {"dateTime": "2026-07-06T09:00:00.0000000", "timeZone": "UTC"},
                    "end": {"dateTime": "2026-07-06T09:30:00.0000000", "timeZone": "UTC"}
                }
            ]
        })
        .to_string();
        let (events, next) = parse_calendar_page(&body).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "id1");
        assert_eq!(next, None);
    }

    #[test]
    fn parses_page_with_next_link() {
        let body = serde_json::json!({
            "value": [],
            "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/calendarView?$skip=50"
        })
        .to_string();
        let (events, next) = parse_calendar_page(&body).unwrap();
        assert!(events.is_empty());
        assert_eq!(
            next,
            Some("https://graph.microsoft.com/v1.0/me/calendarView?$skip=50".to_string())
        );
    }

    #[test]
    fn malformed_page_is_an_error() {
        assert!(parse_calendar_page("not json").is_err());
    }

    #[test]
    fn calendar_view_range_spans_past_and_future() {
        let now = Utc.with_ymd_and_hms(2026, 7, 6, 12, 0, 0).unwrap();
        let (start, end) = calendar_view_range(now, 1, 14);
        assert_eq!(start, "2026-07-05T12:00:00Z");
        assert_eq!(end, "2026-07-20T12:00:00Z");
    }

    #[test]
    fn calendar_view_range_zero_window_collapses_to_now() {
        let now = Utc.with_ymd_and_hms(2026, 7, 6, 0, 0, 0).unwrap();
        let (start, end) = calendar_view_range(now, 0, 0);
        assert_eq!(start, end);
    }
}
