use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use crate::error::AppResult;
use crate::events::ServerEvent;
use crate::models::{CalendarEvent, CalendarPutBody};
use crate::state::AppState;
use crate::time;

pub async fn put_calendar(
    State(state): State<AppState>,
    Json(body): Json<CalendarPutBody>,
) -> AppResult<Json<serde_json::Value>> {
    let now = time::now_iso();
    let mut tx = state.pool.begin().await?;

    for ev in &body.events {
        sqlx::query(
            "INSERT INTO calendar_events (external_id, title, start_at, end_at, place, is_cancelled, received_at) \
             VALUES (?,?,?,?,?,?,?) \
             ON CONFLICT(external_id) DO UPDATE SET \
               title = excluded.title, start_at = excluded.start_at, end_at = excluded.end_at, \
               place = excluded.place, is_cancelled = excluded.is_cancelled, received_at = excluded.received_at",
        )
        .bind(&ev.external_id)
        .bind(&ev.title)
        .bind(&ev.start)
        .bind(&ev.end)
        .bind(&ev.place)
        .bind(ev.is_cancelled)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let today = time::today();
    let events = fetch_day_events(&state.pool, &today).await?;
    state.bus.publish(ServerEvent::CalendarUpdated {
        date: today,
        events,
    });

    Ok(Json(serde_json::json!({ "upserted": body.events.len() })))
}

pub async fn fetch_day_events(pool: &sqlx::SqlitePool, date: &str) -> AppResult<Vec<CalendarEvent>> {
    // start_at is stored as RFC3339 in local wall-clock digits (offset is metadata,
    // not to be re-applied) — substr avoids SQLite's date() reinterpreting via UTC.
    let events: Vec<CalendarEvent> = sqlx::query_as(
        "SELECT id, external_id, title, start_at, end_at, place, is_cancelled, received_at \
         FROM calendar_events \
         WHERE substr(start_at, 1, 10) = ?1 AND is_cancelled = 0 \
         ORDER BY start_at",
    )
    .bind(date)
    .fetch_all(pool)
    .await?;
    Ok(events)
}

#[derive(Debug, Deserialize)]
pub struct CalendarQuery {
    date: Option<String>,
}

pub async fn get_calendar(
    State(state): State<AppState>,
    Query(q): Query<CalendarQuery>,
) -> AppResult<Json<Vec<CalendarEvent>>> {
    let date = q.date.unwrap_or_else(time::today);
    Ok(Json(fetch_day_events(&state.pool, &date).await?))
}
