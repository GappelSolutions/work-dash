use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::error::AppResult;
use crate::events::ServerEvent;
use crate::models::{SetUnreadCount, UnreadCount};
use crate::state::AppState;

/// Sets (not increments) the persisted unread-messages count. The
/// windows-client polls Graph (`/me/chats?$expand=lastMessagePreview,
/// viewpoint`) and diffs against Teams' own "last read" state, so it always
/// pushes the current absolute total — this self-corrects every poll
/// instead of drifting, and needs no "mark read" trigger anywhere in this
/// read-only kiosk system.
pub async fn put_teams(
    State(state): State<AppState>,
    Json(body): Json<SetUnreadCount>,
) -> AppResult<(StatusCode, Json<UnreadCount>)> {
    let count = set_unread_count(&state, body.count).await?;
    Ok((StatusCode::CREATED, Json(UnreadCount { count })))
}

pub async fn set_unread_count(state: &AppState, count: i64) -> AppResult<i64> {
    sqlx::query("UPDATE unread_count SET count = ? WHERE id = 1")
        .bind(count)
        .execute(&state.pool)
        .await?;
    state.bus.publish(ServerEvent::UnreadCountChanged { count });
    Ok(count)
}

pub async fn get_teams(State(state): State<AppState>) -> AppResult<Json<UnreadCount>> {
    Ok(Json(UnreadCount {
        count: get_unread_count(&state.pool).await?,
    }))
}

/// Shared by the JSON API and the CMS info strip.
pub async fn get_unread_count(pool: &sqlx::SqlitePool) -> AppResult<i64> {
    let count: i64 = sqlx::query_scalar("SELECT count FROM unread_count WHERE id = 1")
        .fetch_one(pool)
        .await?;
    Ok(count)
}
