//! Ephemeral "call is ringing" singleton — deliberately not a `teams_events`
//! row: no history, no retention, exactly one active call at a time, gone
//! from the server the instant it's dismissed. See `state::AppState::
//! call_state`.

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::error::AppResult;
use crate::events::ServerEvent;
use crate::models::{CallStatus, PutCallBody};
use crate::state::AppState;

/// Auto-clear fallback: if whatever would normally dismiss the call (the
/// board) is offline or crashed when the call ends, nothing else would ever
/// clear the flag — it would stay "ringing" forever with nothing left to
/// dismiss it. 60s comfortably outlasts a real Teams ring (Teams itself
/// gives up well before that), so it never races a call still genuinely
/// in progress.
const AUTO_CLEAR_AFTER: Duration = Duration::from_secs(60);

pub async fn put_call(
    State(state): State<AppState>,
    Json(body): Json<PutCallBody>,
) -> AppResult<StatusCode> {
    let id = state.call_seq.fetch_add(1, Ordering::SeqCst) + 1;
    {
        let mut guard = state.call_state.lock().unwrap();
        *guard = Some((id, body.caller.clone()));
    }
    state.bus.publish(ServerEvent::CallStateChanged {
        active: true,
        caller: Some(body.caller),
    });

    let bus = state.bus.clone();
    let call_state = state.call_state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(AUTO_CLEAR_AFTER).await;
        let cleared = {
            let mut guard = call_state.lock().unwrap();
            // Only clear if this is still "our" call — a newer PUT (new
            // generation id) since then means a fresh call is ringing and
            // must not be clobbered by this stale timeout.
            if matches!(&*guard, Some((cur_id, _)) if *cur_id == id) {
                *guard = None;
                true
            } else {
                false
            }
        };
        if cleared {
            bus.publish(ServerEvent::CallStateChanged {
                active: false,
                caller: None,
            });
        }
    });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_call(State(state): State<AppState>) -> AppResult<StatusCode> {
    let had_call = {
        let mut guard = state.call_state.lock().unwrap();
        guard.take().is_some()
    };
    if had_call {
        state.bus.publish(ServerEvent::CallStateChanged {
            active: false,
            caller: None,
        });
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_call(State(state): State<AppState>) -> AppResult<Json<CallStatus>> {
    let guard = state.call_state.lock().unwrap();
    Ok(Json(match &*guard {
        Some((_, caller)) => CallStatus {
            active: true,
            caller: Some(caller.clone()),
        },
        None => CallStatus {
            active: false,
            caller: None,
        },
    }))
}
