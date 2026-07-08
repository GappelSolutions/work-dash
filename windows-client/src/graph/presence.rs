//! Polls `/me/presence` as a secondary, confirming signal ("currently in a
//! call") — this alone cannot detect an incoming ring (it only flips to
//! `InACall` once the call is joined), so the primary ring detector is one
//! of `teams::window_win`, `teams::logtail`, or (legacy, non-functional for
//! new Teams) `teams::listener_win`; see `main.rs::start_call_source`.

use serde::Deserialize;

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

pub struct PresenceClient {
    client: reqwest::blocking::Client,
    access_token: String,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct Presence {
    pub availability: String,
    pub activity: String,
}

impl Presence {
    pub fn is_in_call(&self) -> bool {
        matches!(self.activity.as_str(), "InACall" | "InAConferenceCall")
    }
}

/// Parses a `/me/presence` response body. Pure — no I/O.
pub fn parse_presence(body: &str) -> Result<Presence, String> {
    serde_json::from_str(body).map_err(|e| e.to_string())
}

impl PresenceClient {
    pub fn new(access_token: impl Into<String>) -> Self {
        PresenceClient {
            client: reqwest::blocking::Client::new(),
            access_token: access_token.into(),
        }
    }

    pub fn fetch_presence(&self) -> Result<Presence, String> {
        let resp = self
            .client
            .get(format!("{GRAPH_BASE}/me/presence"))
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(format!("presence request failed: {status} {body}"));
        }

        let body = resp.text().map_err(|e| e.to_string())?;
        parse_presence(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_in_a_call_presence() {
        let body = serde_json::json!({
            "availability": "Busy",
            "activity": "InACall"
        })
        .to_string();
        let presence = parse_presence(&body).unwrap();
        assert!(presence.is_in_call());
    }

    #[test]
    fn available_presence_is_not_in_call() {
        let body = serde_json::json!({
            "availability": "Available",
            "activity": "Available"
        })
        .to_string();
        let presence = parse_presence(&body).unwrap();
        assert!(!presence.is_in_call());
    }

    #[test]
    fn in_a_conference_call_counts_as_in_call() {
        let body = serde_json::json!({
            "availability": "Busy",
            "activity": "InAConferenceCall"
        })
        .to_string();
        assert!(parse_presence(&body).unwrap().is_in_call());
    }

    #[test]
    fn in_a_meeting_is_not_in_call() {
        let body = serde_json::json!({
            "availability": "Busy",
            "activity": "InAMeeting"
        })
        .to_string();
        assert!(!parse_presence(&body).unwrap().is_in_call());
    }
}
