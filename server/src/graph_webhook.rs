//! Receives Microsoft Graph change-notification webhook deliveries for
//! Teams chat messages (see windows-client's `graph::subscriptions`, which
//! creates/renews the subscription pointing at this endpoint).
//!
//! Deliberately does not fetch or decrypt message content — the
//! subscription is created without `includeResourceData`, so all a
//! notification carries is "something changed in this chat," which is
//! enough to forward as a generic notification. Public by necessity (Graph
//! calls this with no bearer/cookie auth this server understands); the
//! shared `clientState` secret set on the subscription is the only guard
//! against spoofed deliveries, so treat it like a credential (see
//! `GRAPH_WEBHOOK_CLIENT_STATE` in `README`/deploy config).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::models::{TeamsEventIn, TeamsKind};
use crate::state::AppState;
use crate::teams::insert_teams_event;

#[derive(Debug, Deserialize)]
pub struct ValidationQuery {
    #[serde(rename = "validationToken")]
    validation_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotificationPayload {
    value: Vec<ChangeNotification>,
}

#[derive(Debug, Deserialize)]
struct ChangeNotification {
    #[serde(rename = "subscriptionId")]
    subscription_id: String,
    #[serde(rename = "clientState")]
    client_state: Option<String>,
    resource: Option<String>,
}

pub async fn receive_notifications(
    State(state): State<AppState>,
    Query(q): Query<ValidationQuery>,
    body: String,
) -> Response {
    // Subscription-creation (and periodic renewal) handshake: Graph POSTs
    // with `validationToken` in the query string, empty body, and requires
    // it echoed back verbatim as `text/plain` within ~10s.
    if let Some(token) = q.validation_token {
        return (StatusCode::OK, [("content-type", "text/plain")], token).into_response();
    }

    let Some(expected_state) = state.graph_webhook_client_state.as_deref() else {
        tracing::warn!("received Graph notification but GRAPH_WEBHOOK_CLIENT_STATE is unset, dropping");
        return StatusCode::ACCEPTED.into_response();
    };

    let payload: NotificationPayload = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(?e, "malformed Graph notification body");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    for notification in payload.value {
        if notification.client_state.as_deref() != Some(expected_state) {
            tracing::warn!(
                subscription = %notification.subscription_id,
                "Graph notification with missing/mismatched clientState, ignoring"
            );
            continue;
        }

        let event = TeamsEventIn {
            kind: TeamsKind::Info,
            text: "New Teams chat message".to_string(),
            payload: notification
                .resource
                .map(|r| serde_json::json!({ "resource": r, "source": "graph" })),
        };
        if insert_teams_event(&state, event).await.is_err() {
            tracing::error!("failed to record Graph chat notification");
        }
    }

    // Graph requires a 2xx within ~10s or it treats the delivery as failed
    // and retries with backoff, eventually deleting the subscription.
    StatusCode::ACCEPTED.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::events::EventBus;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use axum::Router;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tower::ServiceExt;

    /// Fresh isolated SQLite file per test (mirrors `server/tests/api.rs`'s
    /// convention) — `sqlite::memory:` isn't safe here since the pool hands
    /// out up to 5 connections and each would get its own separate empty
    /// in-memory database. Returns the `TempDir` too so it isn't dropped
    /// (and the file deleted) while the pool is still using it.
    async fn test_state(client_state: Option<&str>) -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = db::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        let state = AppState {
            pool,
            bus: EventBus::new(),
            api_keys: Arc::new(HashSet::new()),
            session_password: Arc::new("pw".to_string()),
            cookie_key: axum_extra::extract::cookie::Key::from(&[0u8; 64]),
            graph_webhook_client_state: client_state.map(str::to_string),
        };
        (state, dir)
    }

    fn app(state: AppState) -> Router {
        Router::new()
            .route("/api/graph/notifications", post(receive_notifications))
            .with_state(state)
    }

    #[tokio::test]
    async fn validation_handshake_echoes_token() {
        let (state, _dir) = test_state(Some("secret")).await;
        let resp = app(state)
            .oneshot(
                Request::post("/api/graph/notifications?validationToken=abc123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
        assert_eq!(&body[..], b"abc123");
    }

    #[tokio::test]
    async fn matching_client_state_is_recorded() {
        let (state, _dir) = test_state(Some("secret")).await;
        let pool = state.pool.clone();
        let body = serde_json::json!({
            "value": [{
                "subscriptionId": "sub-1",
                "clientState": "secret",
                "resource": "chats('19:abc')/messages('1')"
            }]
        });
        let resp = app(state)
            .oneshot(
                Request::post("/api/graph/notifications")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM teams_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn mismatched_client_state_is_ignored() {
        let (state, _dir) = test_state(Some("secret")).await;
        let pool = state.pool.clone();
        let body = serde_json::json!({
            "value": [{
                "subscriptionId": "sub-1",
                "clientState": "wrong",
                "resource": "chats('19:abc')/messages('1')"
            }]
        });
        let resp = app(state)
            .oneshot(
                Request::post("/api/graph/notifications")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM teams_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn unconfigured_secret_drops_without_erroring() {
        let (state, _dir) = test_state(None).await;
        let resp = app(state)
            .oneshot(
                Request::post("/api/graph/notifications")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"value":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }
}
