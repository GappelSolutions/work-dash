use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use chrono::Utc;

use work_dash_windows_client::config::Config;
use work_dash_windows_client::graph::calendar::{calendar_view_range, CalendarClient};
use work_dash_windows_client::graph::presence::PresenceClient;
use work_dash_windows_client::graph::subscriptions::SubscriptionClient;
use work_dash_windows_client::graph::{auth::GraphAuth, token_cache};
use work_dash_windows_client::mapping::map_graph_event;
use work_dash_windows_client::models::{CalendarEventIn, CalendarPutBody, TeamsEventIn, TeamsKind};
use work_dash_windows_client::push::WorkDashClient;
use work_dash_windows_client::teams::classify::{CallClassifier, IncomingCall};
use work_dash_windows_client::teams::listener_mock::MockCallSource;
#[cfg(windows)]
use work_dash_windows_client::teams::listener_win::ToastCallSource;
#[cfg(windows)]
use work_dash_windows_client::teams::logtail::{default_log_dir, LogTailCallSource};
use work_dash_windows_client::teams::source::CallSource;
#[cfg(windows)]
use work_dash_windows_client::teams::window_win::WindowCallSource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = Config::from_env()?;
    let push_client = WorkDashClient::new(config.server_url.clone(), config.api_key.clone());
    let access_token = obtain_access_token(&config)?;

    {
        let calendar_client = CalendarClient::new(access_token.clone());
        let push_client = push_client.clone();
        let poll_secs = config.calendar_poll_secs;
        let days_past = config.calendar_window_days_past;
        let days_future = config.calendar_window_days_future;
        thread::spawn(move || {
            run_calendar_loop(
                calendar_client,
                push_client,
                poll_secs,
                days_past,
                days_future,
            )
        });
    }

    {
        let presence_client = PresenceClient::new(access_token.clone());
        let push_client = push_client.clone();
        let poll_secs = config.presence_poll_secs;
        thread::spawn(move || run_presence_loop(presence_client, push_client, poll_secs));
    }

    if let (Some(public_url), Some(client_state)) = (
        config.graph_webhook_public_url.clone(),
        config.graph_webhook_client_state.clone(),
    ) {
        let subscription_client = SubscriptionClient::new(access_token.clone());
        thread::spawn(move || {
            run_chat_subscription_loop(subscription_client, public_url, client_state)
        });
    } else {
        tracing::info!(
            "GRAPH_WEBHOOK_PUBLIC_URL/GRAPH_WEBHOOK_CLIENT_STATE not set — \
             skipping Graph chat-notification subscription"
        );
    }

    let classifier = CallClassifier::default();
    let calls_rx = start_call_source(&config, classifier);
    for call in calls_rx {
        let body = TeamsEventIn {
            kind: TeamsKind::Call,
            text: format!("Incoming call — {}", call.caller),
            payload: Some(
                serde_json::json!({ "caller": call.caller, "source": config.teams_call_source }),
            ),
        };
        if let Err(e) = push_client.put_teams(&body) {
            tracing::error!(?e, "failed to push incoming call");
        }
    }

    Ok(())
}

fn obtain_access_token(config: &Config) -> Result<String, String> {
    let auth = GraphAuth::new(config.graph_tenant.clone(), config.graph_client_id.clone());

    if let Some(refresh_token) = token_cache::load_refresh_token() {
        match auth.refresh_access_token(&refresh_token) {
            Ok(token) => {
                if let Some(rt) = &token.refresh_token {
                    let _ = token_cache::save_refresh_token(rt);
                }
                return Ok(token.access_token);
            }
            Err(e) => tracing::warn!(%e, "stored refresh token invalid, signing in interactively"),
        }
    }

    let device_code = auth.request_device_code()?;
    println!("{}", device_code.message);
    let token = auth.poll_for_token(&device_code)?;
    if let Some(rt) = &token.refresh_token {
        let _ = token_cache::save_refresh_token(rt);
    }
    Ok(token.access_token)
}

fn sync_calendar_once(
    calendar_client: &CalendarClient,
    days_past: i64,
    days_future: i64,
) -> Result<Vec<CalendarEventIn>, String> {
    let (start, end) = calendar_view_range(Utc::now(), days_past, days_future);
    let graph_events = calendar_client.fetch_calendar_view(&start, &end)?;
    Ok(graph_events.iter().map(map_graph_event).collect())
}

fn run_calendar_loop(
    calendar_client: CalendarClient,
    push_client: WorkDashClient,
    poll_secs: u64,
    days_past: i64,
    days_future: i64,
) {
    let mut last: Vec<CalendarEventIn> = Vec::new();
    loop {
        match sync_calendar_once(&calendar_client, days_past, days_future) {
            Ok(events) => {
                if work_dash_windows_client::diff::has_changed(&last, &events) {
                    let body = CalendarPutBody {
                        events: events.clone(),
                    };
                    match push_client.put_calendar(&body) {
                        Ok(()) => {
                            tracing::info!(count = events.len(), "pushed calendar update");
                            last = events;
                        }
                        Err(e) => tracing::error!(?e, "failed to push calendar"),
                    }
                }
            }
            Err(e) => tracing::error!(%e, "failed to fetch calendar"),
        }
        thread::sleep(Duration::from_secs(poll_secs));
    }
}

/// Presence can't catch the ring itself (it only flips to `InACall` once
/// joined — see `graph::presence`'s doc comment), so this is a secondary
/// confirmation signal that fires even if the primary `CallSource` (window
/// watcher / log tail / toast) misses the ring, at the cost of `poll_secs`
/// latency and no caller name.
fn run_presence_loop(presence_client: PresenceClient, push_client: WorkDashClient, poll_secs: u64) {
    let mut was_in_call = false;
    loop {
        match presence_client.fetch_presence() {
            Ok(presence) => {
                let in_call = presence.is_in_call();
                if in_call && !was_in_call {
                    tracing::info!("presence: entered a call");
                    let body = TeamsEventIn {
                        kind: TeamsKind::Call,
                        text: "In a call (presence)".to_string(),
                        payload: Some(serde_json::json!({ "source": "presence" })),
                    };
                    if let Err(e) = push_client.put_teams(&body) {
                        tracing::error!(?e, "failed to push presence-detected call");
                    }
                }
                was_in_call = in_call;
            }
            Err(e) => tracing::warn!(%e, "failed to fetch presence"),
        }
        thread::sleep(Duration::from_secs(poll_secs));
    }
}

/// Creates the Graph chat-message subscription on startup, then renews it
/// shortly before each 55-minute expiry for as long as the process runs.
/// Delivery lands on `work-dash-server`'s `/api/graph/notifications`
/// webhook directly — this loop only manages the subscription's lifecycle.
fn run_chat_subscription_loop(
    client: SubscriptionClient,
    public_url: String,
    client_state: String,
) {
    let notification_url = format!(
        "{}/api/graph/notifications",
        public_url.trim_end_matches('/')
    );

    let mut subscription = loop {
        match client.create_chat_subscription(&notification_url, &client_state) {
            Ok(sub) => {
                tracing::info!(id = %sub.id, expires = %sub.expiration, "created Graph chat subscription");
                break sub;
            }
            Err(e) => {
                tracing::error!(%e, "failed to create Graph chat subscription, retrying in 60s");
                thread::sleep(Duration::from_secs(60));
            }
        }
    };

    loop {
        let now = chrono::Utc::now();
        let renew_at = subscription.expiration - chrono::Duration::minutes(5);
        let sleep_secs = (renew_at - now).num_seconds().max(30) as u64;
        thread::sleep(Duration::from_secs(sleep_secs));

        match client.renew_subscription(&subscription.id) {
            Ok(renewed) => {
                tracing::info!(id = %renewed.id, expires = %renewed.expiration, "renewed Graph chat subscription");
                subscription = renewed;
            }
            Err(e) => {
                tracing::error!(%e, "failed to renew Graph chat subscription, recreating");
                match client.create_chat_subscription(&notification_url, &client_state) {
                    Ok(sub) => subscription = sub,
                    Err(e) => {
                        tracing::error!(%e, "failed to recreate Graph chat subscription, retrying in 60s");
                        thread::sleep(Duration::from_secs(60));
                    }
                }
            }
        }
    }
}

/// Picks the incoming-call detector: `window` (default — watches Teams' own
/// ring window via `SetWinEventHook`), `logtail` (tails Teams' local log),
/// `toast` (legacy `UserNotificationListener`, known non-functional for new
/// Teams), or `mock`/non-Windows (scripted, via `MOCK_INCOMING_CALLER`) so
/// the sync pipeline stays exercisable without a real Teams ring.
fn start_call_source(config: &Config, classifier: CallClassifier) -> Receiver<IncomingCall> {
    #[cfg(windows)]
    {
        match config.teams_call_source.as_str() {
            "window" => return WindowCallSource::new(classifier).start(),
            "toast" => return ToastCallSource::new(classifier).start(),
            "logtail" => {
                let dir = default_log_dir().unwrap_or_else(|| {
                    tracing::error!("could not resolve Teams log directory (LOCALAPPDATA unset)");
                    std::path::PathBuf::from(".")
                });
                return LogTailCallSource::new(dir).start();
            }
            "mock" => {}
            other => tracing::warn!(
                source = other,
                "unknown TEAMS_CALL_SOURCE, falling back to mock"
            ),
        }
    }
    let _ = &classifier;
    let _ = config;

    let scripted = std::env::var("MOCK_INCOMING_CALLER")
        .ok()
        .map(|caller| vec![IncomingCall { caller }])
        .unwrap_or_default();
    if scripted.is_empty() {
        tracing::warn!(
            "no real Teams call source on this platform; set MOCK_INCOMING_CALLER to demo one"
        );
    }
    MockCallSource::new(scripted).start()
}
