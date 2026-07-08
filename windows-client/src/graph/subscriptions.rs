//! Manages a Microsoft Graph change-notification subscription for Teams
//! chat messages, so the server can be pinged over a webhook when a chat
//! message arrives instead of relying on client-side toast/window scraping
//! (which new Teams' chat notifications don't reliably support either).
//!
//! Deliberately does NOT request `includeResourceData` — that requires an
//! RSA encryption certificate and decrypting `encryptedContent` on receipt,
//! which is a lot of machinery for a use case that only needs to know "a
//! message arrived," not read its content server-side. The webhook payload
//! still carries which chat/message changed, which is enough for a
//! forwarded notification.
//!
//! `/me/chats/getAllMessages` chat-message subscriptions cap at 60 minutes
//! (Microsoft Graph limit) and need active renewal — see `main.rs`'s
//! subscription-renewal loop.

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";
const CHAT_RESOURCE: &str = "/me/chats/getAllMessages";

/// Graph's documented max lifetime for a chatMessage subscription is 60
/// minutes; renewing a few minutes early avoids racing expiry.
const SUBSCRIPTION_LIFETIME_MINS: i64 = 55;

#[derive(Debug, Clone, PartialEq)]
pub struct Subscription {
    pub id: String,
    pub expiration: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct SubscriptionResponse {
    id: String,
    #[serde(rename = "expirationDateTime")]
    expiration_date_time: String,
}

/// Parses a create/renew subscription response body. Pure — no I/O.
pub fn parse_subscription_response(body: &str) -> Result<Subscription, String> {
    let parsed: SubscriptionResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let expiration = DateTime::parse_from_rfc3339(&parsed.expiration_date_time)
        .map_err(|e| e.to_string())?
        .with_timezone(&Utc);
    Ok(Subscription {
        id: parsed.id,
        expiration,
    })
}

/// Builds the JSON body for creating a chat-message subscription. Pure —
/// takes `now` explicitly so tests don't depend on wall-clock time.
pub fn build_create_body(
    notification_url: &str,
    client_state: &str,
    now: DateTime<Utc>,
) -> serde_json::Value {
    let expiration = now + Duration::minutes(SUBSCRIPTION_LIFETIME_MINS);
    json!({
        "changeType": "created,updated",
        "notificationUrl": notification_url,
        "resource": CHAT_RESOURCE,
        "expirationDateTime": expiration.to_rfc3339(),
        "clientState": client_state,
    })
}

pub fn build_renew_body(now: DateTime<Utc>) -> serde_json::Value {
    let expiration = now + Duration::minutes(SUBSCRIPTION_LIFETIME_MINS);
    json!({ "expirationDateTime": expiration.to_rfc3339() })
}

pub struct SubscriptionClient {
    client: reqwest::blocking::Client,
    access_token: String,
}

impl SubscriptionClient {
    pub fn new(access_token: impl Into<String>) -> Self {
        SubscriptionClient {
            client: reqwest::blocking::Client::new(),
            access_token: access_token.into(),
        }
    }

    pub fn create_chat_subscription(
        &self,
        notification_url: &str,
        client_state: &str,
    ) -> Result<Subscription, String> {
        let body = build_create_body(notification_url, client_state, Utc::now());
        let resp = self
            .client
            .post(format!("{GRAPH_BASE}/subscriptions"))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(format!("create subscription failed: {status} {text}"));
        }
        parse_subscription_response(&resp.text().map_err(|e| e.to_string())?)
    }

    pub fn renew_subscription(&self, id: &str) -> Result<Subscription, String> {
        let body = build_renew_body(Utc::now());
        let resp = self
            .client
            .patch(format!("{GRAPH_BASE}/subscriptions/{id}"))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(format!("renew subscription failed: {status} {text}"));
        }
        parse_subscription_response(&resp.text().map_err(|e| e.to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subscription_response() {
        let body = serde_json::json!({
            "id": "sub-123",
            "resource": "/me/chats/getAllMessages",
            "expirationDateTime": "2026-07-07T12:00:00Z"
        })
        .to_string();
        let sub = parse_subscription_response(&body).unwrap();
        assert_eq!(sub.id, "sub-123");
        assert_eq!(sub.expiration.to_rfc3339(), "2026-07-07T12:00:00+00:00");
    }

    #[test]
    fn create_body_sets_expiration_55_minutes_out() {
        let now = DateTime::parse_from_rfc3339("2026-07-07T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let body = build_create_body("https://example.com/webhook", "secret", now);
        assert_eq!(body["resource"], CHAT_RESOURCE);
        assert_eq!(body["clientState"], "secret");
        assert_eq!(body["expirationDateTime"], "2026-07-07T10:55:00+00:00");
    }

    #[test]
    fn renew_body_only_carries_expiration() {
        let now = DateTime::parse_from_rfc3339("2026-07-07T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let body = build_renew_body(now);
        assert_eq!(body.as_object().unwrap().len(), 1);
        assert_eq!(body["expirationDateTime"], "2026-07-07T10:55:00+00:00");
    }
}
