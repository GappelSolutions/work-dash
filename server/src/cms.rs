use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect};
use axum::Form;
use chrono::NaiveDate;
use maud::{html, Markup, DOCTYPE};
use serde::Deserialize;

use crate::calendar::fetch_day_events;
use crate::error::AppResult;
use crate::icons::icon;
use crate::models::{Category, CreateTask, PatchTask, Phase, Task};
use crate::state::AppState;
use crate::tasks;
use crate::teams::get_unread_count;
use crate::time;

fn redirect_back(date: &str, scope: Option<&str>) -> Redirect {
    let scope = scope.unwrap_or("day");
    Redirect::to(&format!("/?date={date}&scope={scope}"))
}

fn shift_date(date: &str, days: i64) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.checked_add_signed(chrono::Duration::days(days)))
        .map(|d| d.to_string())
        .unwrap_or_else(|| date.to_string())
}

#[derive(Debug, Deserialize)]
pub struct BoardQuery {
    scope: Option<String>,
    date: Option<String>,
    category: Option<String>,
}

pub async fn board_page(
    State(state): State<AppState>,
    Query(q): Query<BoardQuery>,
) -> AppResult<Markup> {
    let today = time::today();
    let scope = q.scope.clone().unwrap_or_else(|| "day".into());
    let date = q.date.clone().unwrap_or_else(|| today.clone());
    let preselect = q
        .category
        .as_deref()
        .and_then(Category::parse)
        .unwrap_or(Category::Urgent);

    let board_tasks = tasks::query_tasks(&state.pool, &scope, Some(&date), false).await?;
    let backlog = tasks::query_tasks(&state.pool, "backlog", None, false).await?;
    let calendar_events = fetch_day_events(&state.pool, &date).await?;
    let unread_count = get_unread_count(&state.pool).await?;

    Ok(render_board(
        &scope,
        &date,
        &today,
        preselect,
        &board_tasks,
        &backlog,
        &calendar_events,
        unread_count,
    ))
}

#[allow(clippy::too_many_arguments)]
fn render_board(
    scope: &str,
    date: &str,
    today: &str,
    preselect: Category,
    board_tasks: &[Task],
    backlog: &[Task],
    calendar_events: &[crate::models::CalendarEvent],
    unread_count: i64,
) -> Markup {
    let prev = shift_date(date, -1);
    let next = shift_date(date, 1);
    let is_today = date == today;

    html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "work-dash · cms" }
                link rel="stylesheet" href="/assets/cms.css";
            }
            body {
                div class="wrap" {
                    header class="top" {
                        div class="brand" { b { "WORK-DASH" } span { "· CMS" } }
                        div class="topright" {
                            nav class="daynav" {
                                a class="nav" href=(format!("/?date={prev}&scope={scope}")) title="previous day" { (icon("chevron-left")) }
                                a class=(if is_today {"today on"} else {"today"}) href=(format!("/?date={today}&scope={scope}")) { "Today" }
                                a class="nav" href=(format!("/?date={next}&scope={scope}")) title="next day" { (icon("chevron-right")) }
                                form method="get" action="/" class="inline" {
                                    input type="hidden" name="scope" value=(scope);
                                    input type="date" name="date" value=(date);
                                    button type="submit" class="nav" title="jump to date" { (icon("calendar")) }
                                }
                            }
                            div class="scope" {
                                a class=(if scope=="day" {"on"} else {""}) href=(format!("/?date={date}&scope=day")) { "Day" }
                                a class=(if scope=="all" {"on"} else {""}) href=(format!("/?date={date}&scope=all")) { "All" }
                                a class=(if scope=="backlog" {"on"} else {""}) href=(format!("/?date={date}&scope=backlog")) { "Backlog" }
                            }
                            form method="post" action="/logout" class="inline" {
                                button type="submit" class="iconbtn" title="log out" { (icon("log-out")) }
                            }
                        }
                    }

                    form id="quickadd" class="add" method="post" action="/ui/tasks" {
                        input type="hidden" name="date" value=(date);
                        input type="hidden" name="scope" value=(scope);
                        span class="plus" { (icon("plus")) }
                        input type="text" name="text" placeholder="new task…" required;
                        select name="category" {
                            @for c in Category::ALL {
                                option value=(c.as_str()) selected[c == preselect] { (c.as_str()) }
                            }
                        }
                        select name="when" {
                            option value="day" selected { (format!("on {date}")) }
                            option value="backlog" { "backlog" }
                        }
                        button type="submit" class="go" { "ADD" }
                    }

                    div class="board" {
                        @for cat in Category::ALL {
                            (render_column(cat, date, scope, board_tasks))
                        }
                    }

                    div class="backlog" {
                        h3 { "BACKLOG" em { "undated · assign a date to schedule" } }
                        div class="tray" {
                            @for t in backlog {
                                (render_backlog_chip(t))
                            }
                        }
                    }

                    div class="strip" {
                        div class="box" {
                            h3 { (icon("calendar")) "TODAY'S CALENDAR" span class="ro" { "read-only · from laptop" } }
                            @for e in calendar_events {
                                div class="evline" {
                                    span class="t" { (format!("{}–{}", &e.start_at[11..16.min(e.start_at.len())], &e.end_at[11..16.min(e.end_at.len())])) }
                                    (e.title)
                                    span class="pl" { (e.place.clone().unwrap_or_default()) }
                                }
                            }
                        }
                        div class="box" {
                            h3 { (icon("phone-incoming")) "TEAMS" span class="ro" { "read-only · live via SSE" } }
                            div class="tline" { (format!("{unread_count} unread messages")) }
                        }
                    }
                }
            }
        }
    }
}

fn render_column(cat: Category, date: &str, scope: &str, all_tasks: &[Task]) -> Markup {
    let cards: Vec<&Task> = all_tasks.iter().filter(|t| t.category == cat).collect();
    html! {
        div class="col" data-c=(cat.as_str()) {
            div class="colhead" {
                span class="name" { (cat.as_str()) }
                span class="count" { (cards.len()) }
            }
            div class="cards" {
                @for t in &cards {
                    (render_card(t, date, scope))
                }
            }
            a class="coladd" href=(format!("/?date={date}&scope={scope}&category={}#quickadd", cat.as_str())) {
                (icon("plus")) "add"
            }
        }
    }
}

fn phase_glyph(p: Phase) -> &'static str {
    match p {
        Phase::Untouched => "\u{25CB}",
        Phase::Wip => "\u{25CF}",
        Phase::Done => "\u{2713}",
    }
}

fn render_card(t: &Task, date: &str, scope: &str) -> Markup {
    html! {
        div class=(if t.phase == Phase::Done {"card done"} else {"card"}) {
            form class="inline phase-form" method="post" action=(format!("/ui/tasks/{}/phase", t.id)) {
                input type="hidden" name="date" value=(date);
                input type="hidden" name="scope" value=(scope);
                button type="submit" class="dot" data-p=(t.phase.as_str()) { (phase_glyph(t.phase)) }
                span class="phase-label" { (t.phase.as_str()) }
            }
            form class="inline edit-text" method="post" action=(format!("/ui/tasks/{}/text", t.id)) {
                input type="hidden" name="date" value=(date);
                input type="hidden" name="scope" value=(scope);
                input type="text" name="text" value=(t.text);
                button type="submit" title="save" { (icon("pencil")) }
            }
            div class="card-actions" {
                form class="inline" method="post" action=(format!("/ui/tasks/{}/category", t.id)) {
                    input type="hidden" name="date" value=(date);
                    input type="hidden" name="scope" value=(scope);
                    select name="category" {
                        @for c in Category::ALL {
                            option value=(c.as_str()) selected[c == t.category] { (c.as_str()) }
                        }
                    }
                    button type="submit" title="move" { (icon("chevron-right")) }
                }
                form class="inline" method="post" action=(format!("/ui/tasks/{}/date", t.id)) {
                    input type="hidden" name="scope" value=(scope);
                    input type="date" name="assigned_date" value=(t.assigned_date.clone().unwrap_or_default());
                    button type="submit" title="set date" { (icon("calendar")) }
                }
                form class="inline" method="post" action=(format!("/ui/tasks/{}/delete", t.id)) {
                    input type="hidden" name="date" value=(date);
                    input type="hidden" name="scope" value=(scope);
                    button type="submit" title="delete" { (icon("trash-2")) }
                }
            }
        }
    }
}

fn render_backlog_chip(t: &Task) -> Markup {
    html! {
        form class="chip-form" method="post" action=(format!("/ui/tasks/{}/date", t.id)) {
            input type="hidden" name="scope" value="backlog";
            span class=(format!("cdot cdot-{}", t.category.as_str())) {}
            span { (t.text) }
            input type="date" name="assigned_date" value=(time::today());
            button type="submit" title="schedule" { (icon("calendar")) }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RedirectForm {
    date: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct QuickAddForm {
    text: String,
    category: Category,
    when: Option<String>,
    date: String,
    scope: Option<String>,
}

pub async fn quick_add(
    State(state): State<AppState>,
    Form(f): Form<QuickAddForm>,
) -> AppResult<impl IntoResponse> {
    let assigned_date = if f.when.as_deref() == Some("backlog") {
        None
    } else {
        Some(f.date.clone())
    };
    tasks::create_task_inner(
        &state,
        CreateTask {
            text: f.text,
            category: f.category,
            assigned_date,
            phase: None,
        },
    )
    .await?;
    Ok(redirect_back(&f.date, f.scope.as_deref()))
}

pub async fn cycle_phase(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(f): Form<RedirectForm>,
) -> AppResult<impl IntoResponse> {
    let current = tasks::get_task(&state.pool, id).await?;
    tasks::patch_task_inner(
        &state,
        id,
        PatchTask {
            phase: Some(current.phase.next()),
            ..Default::default()
        },
    )
    .await?;
    Ok(redirect_back(
        f.date.as_deref().unwrap_or(&time::today()),
        f.scope.as_deref(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CategoryForm {
    category: Category,
    date: String,
    scope: Option<String>,
}

pub async fn set_category(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(f): Form<CategoryForm>,
) -> AppResult<impl IntoResponse> {
    tasks::patch_task_inner(
        &state,
        id,
        PatchTask {
            category: Some(f.category),
            ..Default::default()
        },
    )
    .await?;
    Ok(redirect_back(&f.date, f.scope.as_deref()))
}

#[derive(Debug, Deserialize)]
pub struct DateForm {
    assigned_date: Option<String>,
    date: Option<String>,
    scope: Option<String>,
}

pub async fn set_date(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(f): Form<DateForm>,
) -> AppResult<impl IntoResponse> {
    let new_date = match f.assigned_date.as_deref() {
        Some("") | None => None,
        Some(d) => Some(d.to_string()),
    };
    let redirect_date = f
        .date
        .clone()
        .or_else(|| new_date.clone())
        .unwrap_or_else(time::today);
    tasks::patch_task_inner(
        &state,
        id,
        PatchTask {
            assigned_date: Some(new_date),
            ..Default::default()
        },
    )
    .await?;
    Ok(redirect_back(&redirect_date, f.scope.as_deref()))
}

#[derive(Debug, Deserialize)]
pub struct TextForm {
    text: String,
    date: String,
    scope: Option<String>,
}

pub async fn set_text(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(f): Form<TextForm>,
) -> AppResult<impl IntoResponse> {
    tasks::patch_task_inner(
        &state,
        id,
        PatchTask {
            text: Some(f.text),
            ..Default::default()
        },
    )
    .await?;
    Ok(redirect_back(&f.date, f.scope.as_deref()))
}

pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(f): Form<RedirectForm>,
) -> AppResult<impl IntoResponse> {
    tasks::delete_task_inner(&state, id).await?;
    Ok(redirect_back(
        f.date.as_deref().unwrap_or(&time::today()),
        f.scope.as_deref(),
    ))
}
