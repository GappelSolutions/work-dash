use std::collections::HashSet;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;
use sqlx::SqlitePool;

use crate::events::EventBus;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub bus: EventBus,
    pub api_keys: Arc<HashSet<String>>,
    pub session_password: Arc<String>,
    pub cookie_key: Key,
    /// Shared secret Microsoft Graph echoes back on every chat-notification
    /// webhook delivery (see `graph_webhook`). `None` if the windows-client
    /// hasn't been configured to create a Graph subscription — the webhook
    /// route stays mounted either way but drops everything it receives.
    pub graph_webhook_client_state: Option<String>,
    /// Ephemeral "is a call currently ringing" singleton — `(generation,
    /// caller)`. In-memory only by design (see `call` module): no DB table,
    /// doesn't survive a restart, exactly one call at a time. The generation
    /// id lets the ~60s auto-clear fallback (`call::put_call`) tell "this is
    /// still the call I was scheduled for" apart from a newer one.
    pub call_state: Arc<Mutex<Option<(u64, String)>>>,
    pub call_seq: Arc<AtomicU64>,
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}
