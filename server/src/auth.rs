use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use maud::html;
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;

use crate::state::AppState;

const SESSION_COOKIE: &str = "session";
const SESSION_VALUE: &str = "ok";

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Routes reachable without a session cookie or API key.
///
/// `/api/graph/notifications` is public because Microsoft Graph delivers
/// webhooks with neither a bearer token nor our session cookie — it's
/// guarded instead by the shared `clientState` secret checked inside
/// `graph_webhook::receive_notifications`.
fn is_public(path: &str) -> bool {
    path == "/health"
        || path == "/login"
        || path == "/logout"
        || path.starts_with("/assets/")
        || path == "/api/graph/notifications"
}

pub async fn auth_guard(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if is_public(&path) {
        return next.run(req).await;
    }

    let bearer_ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|token| state.api_keys.iter().any(|k| constant_time_eq(k, token)))
        .unwrap_or(false);

    let cookie_ok = jar
        .get(SESSION_COOKIE)
        .map(|c| c.value() == SESSION_VALUE)
        .unwrap_or(false);

    if bearer_ok || cookie_ok {
        return next.run(req).await;
    }

    let wants_html = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/html"))
        .unwrap_or(false);

    if wants_html {
        Redirect::to("/login").into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()
    }
}

pub async fn login_page() -> impl IntoResponse {
    render_login(None)
}

fn render_login(error: Option<&str>) -> maud::Markup {
    html! {
        (maud::DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                title { "work-dash · login" }
                link rel="stylesheet" href="/assets/cms.css";
            }
            body class="login-body" {
                form class="login-card" method="post" action="/login" {
                    div class="login-brand" { "WORK-DASH" }
                    @if let Some(msg) = error {
                        div class="login-error" { (msg) }
                    }
                    input type="password" name="password" placeholder="password" autofocus;
                    button type="submit" { "SIGN IN" }
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    password: String,
}

pub async fn login_submit(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    if constant_time_eq(&form.password, &state.session_password) {
        let cookie = Cookie::build((SESSION_COOKIE, SESSION_VALUE))
            .path("/")
            .http_only(true)
            .same_site(SameSite::Strict)
            .build();
        (jar.add(cookie), Redirect::to("/")).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, render_login(Some("wrong password"))).into_response()
    }
}

pub async fn logout(jar: SignedCookieJar) -> impl IntoResponse {
    (jar.remove(Cookie::from(SESSION_COOKIE)), Redirect::to("/login"))
}
