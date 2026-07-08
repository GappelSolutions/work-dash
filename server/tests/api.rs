use std::collections::HashSet;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;
use work_dash_server::events::EventBus;
use work_dash_server::state::AppState;

const API_KEY: &str = "test-api-key";
const PASSWORD: &str = "test-password";

/// Fresh isolated SQLite file per test (migrations run) — never shared, so
/// tests can run concurrently without racing each other's data.
async fn test_router() -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite://{}", db_path.display());
    let pool = work_dash_server::db::connect(&url).await.unwrap();

    let mut api_keys = HashSet::new();
    api_keys.insert(API_KEY.to_string());

    let state = AppState {
        pool,
        bus: EventBus::new(),
        api_keys: Arc::new(api_keys),
        session_password: Arc::new(PASSWORD.to_string()),
        cookie_key: work_dash_server::derive_cookie_key("test-secret"),
        graph_webhook_client_state: None,
    };

    (work_dash_server::build_router(state), dir)
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn auth_req(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {API_KEY}"));
    let body = match body {
        Some(v) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

#[tokio::test]
async fn api_without_key_is_rejected() {
    let (app, _dir) = test_router().await;
    let resp = app
        .oneshot(Request::builder().uri("/api/tasks").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn browser_without_session_redirects_to_login() {
    let (app, _dir) = test_router().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
}

#[tokio::test]
async fn login_rejects_wrong_password_and_accepts_right_one() {
    let (app, _dir) = test_router().await;

    let bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("password=nope"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

    let good = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("password={PASSWORD}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(good.status(), StatusCode::SEE_OTHER);
    assert!(good.headers().get(header::SET_COOKIE).is_some());
}

#[tokio::test]
async fn create_task_appears_in_day_scope() {
    let (app, _dir) = test_router().await;

    let create = auth_req(
        "POST",
        "/api/tasks",
        Some(serde_json::json!({
            "text": "Prod alert triage",
            "category": "urgent",
            "assigned_date": "2026-07-05"
        })),
    );
    let resp = app.clone().oneshot(create).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    assert_eq!(created["category"], "urgent");
    assert_eq!(created["phase"], "untouched");

    let list = app
        .oneshot(auth_req(
            "GET",
            "/api/tasks?scope=day&date=2026-07-05",
            None,
        ))
        .await
        .unwrap();
    let tasks = body_json(list).await;
    let arr = tasks.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["text"], "Prod alert triage");
}

#[tokio::test]
async fn patch_writes_history_for_each_changed_field() {
    let (app, _dir) = test_router().await;

    let created = body_json(
        app.clone()
            .oneshot(auth_req(
                "POST",
                "/api/tasks",
                Some(serde_json::json!({"text": "x", "category": "admin"})),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let patch_resp = app
        .clone()
        .oneshot(auth_req(
            "PATCH",
            &format!("/api/tasks/{id}"),
            Some(serde_json::json!({"category": "urgent", "phase": "wip"})),
        ))
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patched = body_json(patch_resp).await;
    assert_eq!(patched["category"], "urgent");
    assert_eq!(patched["phase"], "wip");

    let history = body_json(
        app.oneshot(auth_req("GET", &format!("/api/tasks/{id}/history"), None))
            .await
            .unwrap(),
    )
    .await;
    let actions: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"created"));
    assert!(actions.contains(&"moved"));
    assert!(actions.contains(&"phase_changed"));
}

#[tokio::test]
async fn soft_delete_excludes_then_restore_brings_back() {
    let (app, _dir) = test_router().await;

    let created = body_json(
        app.clone()
            .oneshot(auth_req(
                "POST",
                "/api/tasks",
                Some(serde_json::json!({
                    "text": "delete me", "category": "creative", "assigned_date": "2026-07-05"
                })),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let del = app
        .clone()
        .oneshot(auth_req("DELETE", &format!("/api/tasks/{id}"), None))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::NO_CONTENT);

    let listed = body_json(
        app.clone()
            .oneshot(auth_req(
                "GET",
                "/api/tasks?scope=day&date=2026-07-05",
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed.as_array().unwrap().len(), 0);

    let restore = app
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/api/tasks/{id}/restore"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::OK);

    let listed_again = body_json(
        app.oneshot(auth_req(
            "GET",
            "/api/tasks?scope=day&date=2026-07-05",
            None,
        ))
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(listed_again.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn calendar_put_upsert_is_idempotent_on_external_id() {
    let (app, _dir) = test_router().await;

    let put_body = serde_json::json!({
        "events": [{
            "external_id": "graph-ev-1",
            "title": "Standup",
            "start": "2026-07-05T09:15:00+02:00",
            "end": "2026-07-05T09:30:00+02:00",
            "place": "Teams"
        }]
    });

    for _ in 0..2 {
        let resp = app
            .clone()
            .oneshot(auth_req("PUT", "/api/calendar", Some(put_body.clone())))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let list = body_json(
        app.oneshot(auth_req("GET", "/api/calendar?date=2026-07-05", None))
            .await
            .unwrap(),
    )
    .await;
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 1, "PUTting the same external_id twice must not duplicate rows");
    assert_eq!(arr[0]["title"], "Standup");
}

#[tokio::test]
async fn teams_put_creates_event_and_get_respects_limit_and_order() {
    let (app, _dir) = test_router().await;

    for i in 0..3 {
        let resp = app
            .clone()
            .oneshot(auth_req(
                "PUT",
                "/api/teams",
                Some(serde_json::json!({"kind": "info", "text": format!("event {i}")})),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    let list = body_json(
        app.oneshot(auth_req("GET", "/api/teams?limit=2", None))
            .await
            .unwrap(),
    )
    .await;
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 2, "limit=2 must cap the result");
    assert_eq!(arr[0]["text"], "event 2", "newest first");
}
