use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::events::ServerEvent;
use crate::models::{CreateTask, HistoryEntry, PatchTask, Phase, Task, TaskRow};
use crate::state::AppState;
use crate::time;

const SELECT_COLS: &str =
    "id,text,category,phase,assigned_date,position,created_at,updated_at,completed_at";

async fn fetch_task_row(pool: &SqlitePool, id: i64) -> AppResult<TaskRow> {
    let sql = format!("SELECT {SELECT_COLS} FROM tasks WHERE id = ?");
    sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    scope: Option<String>,
    date: Option<String>,
    include_deleted: Option<bool>,
}

pub async fn list_tasks(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<Task>>> {
    let scope = q.scope.as_deref().unwrap_or("day");
    let tasks = query_tasks(
        &state.pool,
        scope,
        q.date.as_deref(),
        q.include_deleted.unwrap_or(false),
    )
    .await?;
    Ok(Json(tasks))
}

/// Shared by the JSON API and the CMS board — one place that knows how
/// `scope=day|all|backlog` maps to SQL.
pub async fn query_tasks(
    pool: &SqlitePool,
    scope: &str,
    date: Option<&str>,
    include_deleted: bool,
) -> AppResult<Vec<Task>> {
    let deleted_clause = if include_deleted {
        "1=1"
    } else {
        "deleted_at IS NULL"
    };

    let rows: Vec<TaskRow> = match scope {
        "all" => {
            let sql = format!(
                "SELECT {SELECT_COLS} FROM tasks WHERE {deleted_clause} ORDER BY category, position"
            );
            sqlx::query_as(&sql).fetch_all(pool).await?
        }
        "backlog" => {
            let sql = format!(
                "SELECT {SELECT_COLS} FROM tasks WHERE assigned_date IS NULL AND {deleted_clause} ORDER BY category, position"
            );
            sqlx::query_as(&sql).fetch_all(pool).await?
        }
        _ => {
            let date = date.map(str::to_string).unwrap_or_else(time::today);
            let sql = format!(
                "SELECT {SELECT_COLS} FROM tasks WHERE assigned_date = ? AND {deleted_clause} ORDER BY category, position"
            );
            sqlx::query_as(&sql).bind(date).fetch_all(pool).await?
        }
    };

    Ok(rows.into_iter().map(Task::from).collect())
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(input): Json<CreateTask>,
) -> AppResult<(StatusCode, Json<Task>)> {
    let task = create_task_inner(&state, input).await?;
    Ok((StatusCode::CREATED, Json(task)))
}

/// Shared by the JSON API and the CMS quick-add form.
pub async fn create_task_inner(state: &AppState, input: CreateTask) -> AppResult<Task> {
    if input.text.trim().is_empty() {
        return Err(AppError::BadRequest("text must not be empty".into()));
    }
    let now = time::now_iso();
    let phase = input.phase.unwrap_or(Phase::Untouched);

    let mut tx = state.pool.begin().await?;

    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM tasks \
         WHERE category = ? AND assigned_date IS ? AND deleted_at IS NULL",
    )
    .bind(input.category.as_str())
    .bind(&input.assigned_date)
    .fetch_one(&mut *tx)
    .await?;

    let id = sqlx::query(
        "INSERT INTO tasks (text, category, phase, assigned_date, position, created_at, updated_at) \
         VALUES (?,?,?,?,?,?,?)",
    )
    .bind(&input.text)
    .bind(input.category.as_str())
    .bind(phase.as_str())
    .bind(&input.assigned_date)
    .bind(position)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();

    sqlx::query(
        "INSERT INTO task_history (task_id, action, field, old_value, new_value, changed_at) \
         VALUES (?, 'created', NULL, NULL, ?, ?)",
    )
    .bind(id)
    .bind(&input.text)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let task: Task = fetch_task_row(&state.pool, id).await?.into();
    state.bus.publish(ServerEvent::TaskUpserted { task: task.clone() });
    Ok(task)
}

/// Used by the CMS form handlers (e.g. to compute the next phase before patching).
pub async fn get_task(pool: &SqlitePool, id: i64) -> AppResult<Task> {
    Ok(fetch_task_row(pool, id).await?.into())
}

pub async fn get_task_history(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Vec<HistoryEntry>>> {
    let rows: Vec<HistoryEntry> = sqlx::query_as(
        "SELECT action, field, old_value, new_value, changed_at FROM task_history \
         WHERE task_id = ? ORDER BY changed_at DESC",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn patch_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<PatchTask>,
) -> AppResult<Json<Task>> {
    let task = patch_task_inner(&state, id, patch).await?;
    Ok(Json(task))
}

/// Shared by the JSON API and the CMS card menu (phase cycle, move, date, edit).
pub async fn patch_task_inner(state: &AppState, id: i64, patch: PatchTask) -> AppResult<Task> {
    let current = fetch_task_row(&state.pool, id).await?;
    let now = time::now_iso();
    let mut tx = state.pool.begin().await?;

    if let Some(new_text) = &patch.text {
        if *new_text != current.text {
            sqlx::query("UPDATE tasks SET text = ?, updated_at = ? WHERE id = ?")
                .bind(new_text)
                .bind(&now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO task_history (task_id, action, field, old_value, new_value, changed_at) \
                 VALUES (?, 'updated', 'text', ?, ?, ?)",
            )
            .bind(id)
            .bind(&current.text)
            .bind(new_text)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
    }

    if let Some(new_cat) = patch.category {
        if new_cat.as_str() != current.category {
            sqlx::query("UPDATE tasks SET category = ?, updated_at = ? WHERE id = ?")
                .bind(new_cat.as_str())
                .bind(&now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO task_history (task_id, action, field, old_value, new_value, changed_at) \
                 VALUES (?, 'moved', 'category', ?, ?, ?)",
            )
            .bind(id)
            .bind(&current.category)
            .bind(new_cat.as_str())
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
    }

    if let Some(new_phase) = patch.phase {
        if new_phase.as_str() != current.phase {
            let completed_at = if new_phase == Phase::Done {
                Some(now.clone())
            } else {
                None
            };
            sqlx::query("UPDATE tasks SET phase = ?, completed_at = ?, updated_at = ? WHERE id = ?")
                .bind(new_phase.as_str())
                .bind(&completed_at)
                .bind(&now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO task_history (task_id, action, field, old_value, new_value, changed_at) \
                 VALUES (?, 'phase_changed', 'phase', ?, ?, ?)",
            )
            .bind(id)
            .bind(&current.phase)
            .bind(new_phase.as_str())
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
    }

    if let Some(new_date) = patch.assigned_date.clone() {
        if new_date != current.assigned_date {
            sqlx::query("UPDATE tasks SET assigned_date = ?, updated_at = ? WHERE id = ?")
                .bind(&new_date)
                .bind(&now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO task_history (task_id, action, field, old_value, new_value, changed_at) \
                 VALUES (?, 'dated', 'assigned_date', ?, ?, ?)",
            )
            .bind(id)
            .bind(&current.assigned_date)
            .bind(&new_date)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
    }

    if let Some(new_pos) = patch.position {
        sqlx::query("UPDATE tasks SET position = ?, updated_at = ? WHERE id = ?")
            .bind(new_pos)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    let task: Task = fetch_task_row(&state.pool, id).await?.into();
    state.bus.publish(ServerEvent::TaskUpserted { task: task.clone() });
    Ok(task)
}

pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    delete_task_inner(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_task_inner(state: &AppState, id: i64) -> AppResult<()> {
    // Ensures the task exists (and isn't already gone) before soft-deleting.
    fetch_task_row(&state.pool, id).await?;
    let now = time::now_iso();
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE tasks SET deleted_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO task_history (task_id, action, field, old_value, new_value, changed_at) \
         VALUES (?, 'deleted', NULL, NULL, NULL, ?)",
    )
    .bind(id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    state.bus.publish(ServerEvent::TaskDeleted { id });
    Ok(())
}

pub async fn restore_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Task>> {
    let now = time::now_iso();
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query("UPDATE tasks SET deleted_at = NULL, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query(
        "INSERT INTO task_history (task_id, action, field, old_value, new_value, changed_at) \
         VALUES (?, 'restored', NULL, NULL, NULL, ?)",
    )
    .bind(id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let task: Task = fetch_task_row(&state.pool, id).await?.into();
    state.bus.publish(ServerEvent::TaskUpserted { task: task.clone() });
    Ok(Json(task))
}
