//! Environment-driven configuration. Pure parsing — no I/O beyond `std::env`.

/// Microsoft Graph CLI's well-known public client id. Works with device-code
/// flow without an Azure app registration; override via `GRAPH_CLIENT_ID` if
/// the tenant needs an owned app instead.
const DEFAULT_GRAPH_CLIENT_ID: &str = "14d82eec-204b-4c2f-b7e8-296a70dab67e";
const DEFAULT_GRAPH_TENANT: &str = "organizations";
const DEFAULT_CALENDAR_POLL_SECS: u64 = 300;
const DEFAULT_PRESENCE_POLL_SECS: u64 = 60;
const DEFAULT_CALENDAR_WINDOW_DAYS_PAST: i64 = 1;
const DEFAULT_CALENDAR_WINDOW_DAYS_FUTURE: i64 = 14;
// `logtail` not `window`: calibration on a real box showed new Teams renders
// the ring popup's caller text inside a WebView2 (Chrome_WidgetWin children)
// whose GetWindowText is empty, so the window watcher can't read it without
// full UI Automation — whereas the log carries a reliable call-start marker.
const DEFAULT_TEAMS_CALL_SOURCE: &str = "logtail";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub server_url: String,
    pub api_key: String,
    pub graph_client_id: String,
    pub graph_tenant: String,
    pub calendar_poll_secs: u64,
    pub presence_poll_secs: u64,
    pub calendar_window_days_past: i64,
    pub calendar_window_days_future: i64,
    /// One of `window` (default — watches Teams' own ring window),
    /// `logtail` (tails Teams' local log for call-start markers), `toast`
    /// (legacy `UserNotificationListener`, known non-functional for new
    /// Teams — see `teams::listener_win`), or `mock`.
    pub teams_call_source: String,
    /// Public HTTPS base URL Microsoft Graph can reach to deliver chat
    /// change-notification webhooks, e.g. `https://workdash.gappel.com`.
    /// Chat-notification forwarding (M3) is skipped entirely if unset.
    pub graph_webhook_public_url: Option<String>,
    /// Shared secret echoed back by Graph on every webhook delivery so the
    /// server can reject notifications that didn't originate from our own
    /// subscription. Required if `graph_webhook_public_url` is set.
    pub graph_webhook_client_state: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    MissingServerUrl,
    MissingApiKey,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::MissingServerUrl => write!(f, "WORK_DASH_SERVER_URL must be set"),
            ConfigError::MissingApiKey => write!(f, "WORK_DASH_API_KEY must be set"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Takes a lookup function so tests don't have to mutate real process
    /// env vars (which is unsound to do concurrently across `cargo test`
    /// threads).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
        let server_url = lookup("WORK_DASH_SERVER_URL").ok_or(ConfigError::MissingServerUrl)?;
        let api_key = lookup("WORK_DASH_API_KEY").ok_or(ConfigError::MissingApiKey)?;

        let parse_u64 = |key: &str, default: u64| {
            lookup(key)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(default)
        };
        let parse_i64 = |key: &str, default: i64| {
            lookup(key)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(default)
        };

        Ok(Config {
            server_url: server_url.trim_end_matches('/').to_string(),
            api_key,
            graph_client_id: lookup("GRAPH_CLIENT_ID")
                .unwrap_or_else(|| DEFAULT_GRAPH_CLIENT_ID.to_string()),
            graph_tenant: lookup("GRAPH_TENANT").unwrap_or_else(|| DEFAULT_GRAPH_TENANT.to_string()),
            calendar_poll_secs: parse_u64("CALENDAR_POLL_SECS", DEFAULT_CALENDAR_POLL_SECS),
            presence_poll_secs: parse_u64("PRESENCE_POLL_SECS", DEFAULT_PRESENCE_POLL_SECS),
            calendar_window_days_past: parse_i64(
                "CALENDAR_WINDOW_DAYS_PAST",
                DEFAULT_CALENDAR_WINDOW_DAYS_PAST,
            ),
            calendar_window_days_future: parse_i64(
                "CALENDAR_WINDOW_DAYS_FUTURE",
                DEFAULT_CALENDAR_WINDOW_DAYS_FUTURE,
            ),
            teams_call_source: lookup("TEAMS_CALL_SOURCE")
                .unwrap_or_else(|| DEFAULT_TEAMS_CALL_SOURCE.to_string()),
            graph_webhook_public_url: lookup("GRAPH_WEBHOOK_PUBLIC_URL"),
            graph_webhook_client_state: lookup("GRAPH_WEBHOOK_CLIENT_STATE"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup_from<'a>(map: &'a HashMap<&str, &str>) -> impl Fn(&str) -> Option<String> + 'a {
        move |k: &str| map.get(k).map(|v| v.to_string())
    }

    #[test]
    fn missing_server_url_is_an_error() {
        let map = HashMap::from([("WORK_DASH_API_KEY", "key")]);
        assert_eq!(
            Config::from_lookup(lookup_from(&map)),
            Err(ConfigError::MissingServerUrl)
        );
    }

    #[test]
    fn missing_api_key_is_an_error() {
        let map = HashMap::from([("WORK_DASH_SERVER_URL", "http://localhost:8080")]);
        assert_eq!(
            Config::from_lookup(lookup_from(&map)),
            Err(ConfigError::MissingApiKey)
        );
    }

    #[test]
    fn trailing_slash_on_server_url_is_stripped() {
        let map = HashMap::from([
            ("WORK_DASH_SERVER_URL", "http://localhost:8080/"),
            ("WORK_DASH_API_KEY", "key"),
        ]);
        let config = Config::from_lookup(lookup_from(&map)).unwrap();
        assert_eq!(config.server_url, "http://localhost:8080");
    }

    #[test]
    fn defaults_are_applied_when_optional_vars_absent() {
        let map = HashMap::from([
            ("WORK_DASH_SERVER_URL", "http://localhost:8080"),
            ("WORK_DASH_API_KEY", "key"),
        ]);
        let config = Config::from_lookup(lookup_from(&map)).unwrap();
        assert_eq!(config.graph_client_id, DEFAULT_GRAPH_CLIENT_ID);
        assert_eq!(config.graph_tenant, DEFAULT_GRAPH_TENANT);
        assert_eq!(config.calendar_poll_secs, DEFAULT_CALENDAR_POLL_SECS);
        assert_eq!(config.presence_poll_secs, DEFAULT_PRESENCE_POLL_SECS);
        assert_eq!(
            config.calendar_window_days_past,
            DEFAULT_CALENDAR_WINDOW_DAYS_PAST
        );
        assert_eq!(
            config.calendar_window_days_future,
            DEFAULT_CALENDAR_WINDOW_DAYS_FUTURE
        );
        assert_eq!(config.teams_call_source, "logtail");
        assert_eq!(config.graph_webhook_public_url, None);
        assert_eq!(config.graph_webhook_client_state, None);
    }

    #[test]
    fn overrides_are_honored() {
        let map = HashMap::from([
            ("WORK_DASH_SERVER_URL", "http://localhost:8080"),
            ("WORK_DASH_API_KEY", "key"),
            ("GRAPH_CLIENT_ID", "custom-client-id"),
            ("GRAPH_TENANT", "contoso.onmicrosoft.com"),
            ("CALENDAR_POLL_SECS", "60"),
            ("PRESENCE_POLL_SECS", "10"),
            ("CALENDAR_WINDOW_DAYS_PAST", "2"),
            ("CALENDAR_WINDOW_DAYS_FUTURE", "30"),
        ]);
        let config = Config::from_lookup(lookup_from(&map)).unwrap();
        assert_eq!(config.graph_client_id, "custom-client-id");
        assert_eq!(config.graph_tenant, "contoso.onmicrosoft.com");
        assert_eq!(config.calendar_poll_secs, 60);
        assert_eq!(config.presence_poll_secs, 10);
        assert_eq!(config.calendar_window_days_past, 2);
        assert_eq!(config.calendar_window_days_future, 30);
    }

    #[test]
    fn unparseable_numeric_override_falls_back_to_default() {
        let map = HashMap::from([
            ("WORK_DASH_SERVER_URL", "http://localhost:8080"),
            ("WORK_DASH_API_KEY", "key"),
            ("CALENDAR_POLL_SECS", "not-a-number"),
        ]);
        let config = Config::from_lookup(lookup_from(&map)).unwrap();
        assert_eq!(config.calendar_poll_secs, DEFAULT_CALENDAR_POLL_SECS);
    }
}
