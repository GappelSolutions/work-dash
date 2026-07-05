use std::collections::HashSet;
use std::sync::Arc;

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
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}
