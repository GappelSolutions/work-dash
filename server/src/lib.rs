pub mod auth;
pub mod calendar;
pub mod call;
pub mod cms;
pub mod db;
pub mod error;
pub mod events;
pub mod graph_webhook;
pub mod icons;
pub mod models;
pub mod sse;
pub mod state;
pub mod tasks;
pub mod teams;
pub mod time;

use std::collections::HashSet;
use std::sync::Arc;

use axum::routing::{get, patch, post};
use axum::Router;
use axum_extra::extract::cookie::Key;
use sha2::{Digest, Sha512};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use state::AppState;

/// `cookie::Key::from` requires a 64-byte key; hash an arbitrary-length
/// SESSION_SECRET into exactly that size rather than requiring the operator
/// to hand-generate 64 bytes of entropy.
pub fn derive_cookie_key(secret: &str) -> Key {
    let mut hasher = Sha512::new();
    hasher.update(secret.as_bytes());
    Key::from(&hasher.finalize())
}

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route(
            "/tasks/{id}",
            patch(tasks::patch_task).delete(tasks::delete_task),
        )
        .route("/tasks/{id}/restore", post(tasks::restore_task))
        .route("/tasks/{id}/history", get(tasks::get_task_history))
        .route(
            "/calendar",
            get(calendar::get_calendar).put(calendar::put_calendar),
        )
        .route("/teams", get(teams::get_teams).put(teams::put_teams))
        .route(
            "/call",
            get(call::get_call).put(call::put_call).delete(call::clear_call),
        )
        .route("/events", get(sse::sse_handler))
        .route(
            "/graph/notifications",
            post(graph_webhook::receive_notifications),
        );

    Router::new()
        .route("/", get(cms::board_page))
        .route("/ui/tasks", post(cms::quick_add))
        .route("/ui/tasks/{id}/phase", post(cms::cycle_phase))
        .route("/ui/tasks/{id}/category", post(cms::set_category))
        .route("/ui/tasks/{id}/date", post(cms::set_date))
        .route("/ui/tasks/{id}/text", post(cms::set_text))
        .route("/ui/tasks/{id}/delete", post(cms::delete_task))
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route("/logout", post(auth::logout))
        .route("/health", get(|| async { "ok" }))
        .nest("/api", api)
        .nest_service("/assets", ServeDir::new("assets"))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_guard,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub fn parse_api_keys(raw: &str) -> HashSet<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://work-dash.db".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let session_password =
        std::env::var("SESSION_PASSWORD").expect("SESSION_PASSWORD must be set");
    let session_secret = std::env::var("SESSION_SECRET").expect("SESSION_SECRET must be set");
    let api_keys = parse_api_keys(&std::env::var("API_KEYS").unwrap_or_default());
    let graph_webhook_client_state = std::env::var("GRAPH_WEBHOOK_CLIENT_STATE").ok();

    let pool = db::connect(&database_url).await?;

    let state = AppState {
        pool,
        bus: events::EventBus::new(),
        api_keys: Arc::new(api_keys),
        session_password: Arc::new(session_password),
        cookie_key: derive_cookie_key(&session_secret),
        graph_webhook_client_state,
        call_state: Arc::new(std::sync::Mutex::new(None)),
        call_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("work-dash-server listening on 0.0.0.0:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Without this, `docker stop`/`podman stop` sends SIGTERM, gets no
/// response, and has to wait out the timeout before SIGKILL.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
