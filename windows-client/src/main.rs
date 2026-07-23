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
use work_dash_windows_client::models::{CalendarEventIn, CalendarPutBody};
use work_dash_windows_client::push::WorkDashClient;
use work_dash_windows_client::teams::classify::{CallClassifier, IncomingCall};
use work_dash_windows_client::teams::listener_mock::MockCallSource;
#[cfg(windows)]
use work_dash_windows_client::teams::listener_win::ToastCallSource;
#[cfg(windows)]
use work_dash_windows_client::teams::logtail::{default_log_dir, LogTailCallSource};
use work_dash_windows_client::teams::source::CallSource;
#[cfg(windows)]
use work_dash_windows_client::teams::unread::UnreadCountSource;
#[cfg(windows)]
use work_dash_windows_client::teams::window_win::WindowCallSource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = Config::from_env()?;
    let push_client = WorkDashClient::new(config.server_url.clone(), config.api_key.clone());

    // Call detection and the unread-count tail are both local-only (window
    // watcher / log tail) and never touch Graph — they must keep working
    // even where Microsoft sign-in is unavailable (blocked tenant, no admin
    // consent, etc.), so both start unconditionally, before any Graph call.
    start_unread_loop(&push_client);

    // Calendar/presence/chat-subscription are Graph-dependent and therefore
    // best-effort only: calendar is already covered without Graph by
    // `outlook_calendar_push.ps1` (Outlook COM automation, no sign-in), so a
    // failure here just means those three loops don't run — it must not
    // take down call detection or the unread tail with it.
    match obtain_access_token(&config) {
        Ok(access_token) => start_graph_loops(&config, &push_client, access_token),
        Err(e) => tracing::warn!(
            %e,
            "Graph sign-in unavailable — running without it (calendar via \
             outlook_calendar_push.ps1, unread count via local log tail)"
        ),
    }

    let classifier = CallClassifier::default();
    let calls_rx = start_call_source(&config, classifier);
    for call in calls_rx {
        if let Err(e) = push_client.put_call(&call.caller) {
            tracing::error!(?e, "failed to push incoming call");
        }
    }

    Ok(())
}

fn start_graph_loops(config: &Config, push_client: &WorkDashClient, access_token: String) {
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
        let subscription_client = SubscriptionClient::new(access_token);
        thread::spawn(move || {
            run_chat_subscription_loop(subscription_client, public_url, client_state)
        });
    } else {
        tracing::info!(
            "GRAPH_WEBHOOK_PUBLIC_URL/GRAPH_WEBHOOK_CLIENT_STATE not set — \
             skipping Graph chat-notification subscription"
        );
    }
}

/// Tails Teams' own local log for its unread-badge marker (see
/// `teams::unread`) and forwards each change straight to the server — no
/// Graph, no local read/unread bookkeeping. Windows-only, same as the
/// `logtail`/`window_win` call sources it shares its tailing machinery with.
fn start_unread_loop(push_client: &WorkDashClient) {
    #[cfg(windows)]
    {
        let dir = match default_log_dir() {
            Some(dir) => dir,
            None => {
                tracing::error!(
                    "could not resolve Teams log directory (LOCALAPPDATA unset), \
                     unread count will not be tracked"
                );
                return;
            }
        };
        let push_client = push_client.clone();
        let rx = UnreadCountSource::new(dir).start();
        thread::spawn(move || {
            for count in rx {
                if let Err(e) = push_client.set_unread_count(count) {
                    tracing::error!(?e, "failed to push unread count");
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = push_client;
        tracing::warn!("unread-count log tail is Windows-only; not running on this platform");
    }
}

/// Only ever uses a cached refresh token — deliberately does **not** fall
/// back to interactive device-code sign-in. That flow blocks synchronously
/// waiting for someone to complete it in a browser, which would hang
/// `main()` (and with it call detection/unread tracking, neither of which
/// need Graph at all) on every startup in environments where Microsoft
/// sign-in isn't available. If you need Graph enabled, run the interactive
/// login once out-of-band (e.g. via a short-lived helper) so a refresh
/// token lands in the credential cache; this just consumes it.
fn obtain_access_token(config: &Config) -> Result<String, String> {
    let auth = GraphAuth::new(config.graph_tenant.clone(), config.graph_client_id.clone());

    let refresh_token = token_cache::load_refresh_token()
        .ok_or_else(|| "no cached Graph refresh token (interactive sign-in required, not attempted automatically)".to_string())?;

    let token = auth.refresh_access_token(&refresh_token)?;
    if let Some(rt) = &token.refresh_token {
        let _ = token_cache::save_refresh_token(rt);
    }
    Ok(token.access_token)
}

fn sync_calendar_once(
    calendar_client: &CalendarClient,
    days_past: i64,
    days_future: i64,
) -> Result<(String, String, Vec<CalendarEventIn>), String> {
    let (start, end) = calendar_view_range(Utc::now(), days_past, days_future);
    let graph_events = calendar_client.fetch_calendar_view(&start, &end)?;
    let events = graph_events.iter().map(map_graph_event).collect();
    Ok((start, end, events))
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
            Ok((range_start, range_end, events)) => {
                if work_dash_windows_client::diff::has_changed(&last, &events) {
                    let body = CalendarPutBody {
                        events: events.clone(),
                        range_start,
                        range_end,
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
                    // No caller name available from presence alone — this
                    // only fires as a fallback when the primary ring
                    // detector (window/logtail/toast) missed the ring
                    // entirely, so a generic banner beats none. The
                    // server's ~60s auto-clear timeout takes it back down
                    // since there's no explicit "call ended" signal here.
                    tracing::info!("presence: entered a call the primary detector missed");
                    if let Err(e) = push_client.put_call("Unknown caller (presence)") {
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
