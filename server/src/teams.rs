use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::error::AppResult;
use crate::events::ServerEvent;
use crate::models::{TeamsEvent, TeamsEventIn, TeamsEventRow};
use crate::state::AppState;
use crate::time;

pub async fn put_teams(
    State(state): State<AppState>,
    Json(input): Json<TeamsEventIn>,
) -> AppResult<(StatusCode, Json<TeamsEvent>)> {
    let now = time::now_iso();
    let payload_json = input.payload.as_ref().map(|p| p.to_string());

    let id = sqlx::query(
        "INSERT INTO teams_events (kind, text, payload, created_at) VALUES (?,?,?,?)",
    )
    .bind(input.kind.as_str())
    .bind(&input.text)
    .bind(&payload_json)
    .bind(&now)
    .execute(&state.pool)
    .await?
    .last_insert_rowid();

    let row: TeamsEventRow = sqlx::query_as(
        "SELECT id, kind, text, payload, created_at FROM teams_events WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    let event: TeamsEvent = row.into();
    state
        .bus
        .publish(ServerEvent::TeamsEventFired(event.clone()));
    Ok((StatusCode::CREATED, Json(event)))
}

#[derive(Debug, Deserialize)]
pub struct TeamsQuery {
    limit: Option<i64>,
}

pub async fn get_teams(
    State(state): State<AppState>,
    Query(q): Query<TeamsQuery>,
) -> AppResult<Json<Vec<TeamsEvent>>> {
    let limit = q.limit.unwrap_or(10).clamp(1, 100);
    Ok(Json(fetch_recent_teams(&state.pool, limit).await?))
}

/// Shared by the JSON API and the CMS info strip.
pub async fn fetch_recent_teams(pool: &sqlx::SqlitePool, limit: i64) -> AppResult<Vec<TeamsEvent>> {
    let rows: Vec<TeamsEventRow> = sqlx::query_as(
        "SELECT id, kind, text, payload, created_at FROM teams_events \
         ORDER BY created_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TeamsEvent::from).collect())
}
