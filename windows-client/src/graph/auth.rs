//! Device-code OAuth flow against Microsoft's identity platform, using the
//! Graph CLI's well-known public client id (no Azure app registration
//! required — overridable via config for tenants that need an owned app).
//!
//! Response parsing is pure and unit-tested; the actual HTTP calls
//! (`request_device_code`, `poll_for_token`, `refresh_access_token`) are
//! thin wrappers so integration is a one-line swap in tests.

use std::time::Duration;

use serde::Deserialize;

const SCOPES: &str = "Calendars.Read Presence.Read Chat.Read offline_access User.Read";

pub struct GraphAuth {
    client: reqwest::blocking::Client,
    tenant: String,
    client_id: String,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: String,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

#[derive(Debug, PartialEq, Clone)]
pub enum TokenPollOutcome {
    Success(TokenResponse),
    /// User hasn't finished signing in yet — caller should sleep `interval`
    /// seconds and poll again.
    Pending,
    /// Terminal failure (expired, denied, or any other OAuth error).
    Error(String),
}

/// Parses the raw JSON body of a `POST .../oauth2/v2.0/token` response.
/// Distinguishes `authorization_pending` (keep polling) from every other
/// error (give up) and from a successful token grant.
pub fn parse_token_poll_response(status: u16, body: &str) -> TokenPollOutcome {
    if status == 200 {
        return match serde_json::from_str::<TokenResponse>(body) {
            Ok(t) => TokenPollOutcome::Success(t),
            Err(e) => TokenPollOutcome::Error(format!("malformed token response: {e}")),
        };
    }

    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }

    match serde_json::from_str::<ErrorBody>(body) {
        Ok(e) if e.error == "authorization_pending" => TokenPollOutcome::Pending,
        Ok(e) if e.error == "slow_down" => TokenPollOutcome::Pending,
        Ok(e) => TokenPollOutcome::Error(e.error),
        Err(e) => TokenPollOutcome::Error(format!("malformed error response: {e}")),
    }
}

pub fn parse_device_code_response(body: &str) -> Result<DeviceCodeResponse, serde_json::Error> {
    serde_json::from_str(body)
}

impl GraphAuth {
    pub fn new(tenant: impl Into<String>, client_id: impl Into<String>) -> Self {
        GraphAuth {
            client: reqwest::blocking::Client::new(),
            tenant: tenant.into(),
            client_id: client_id.into(),
        }
    }

    fn authority(&self) -> String {
        format!("https://login.microsoftonline.com/{}/oauth2/v2.0", self.tenant)
    }

    /// Starts the device-code flow. Prints `message` to the caller so the
    /// user can go sign in in a browser.
    pub fn request_device_code(&self) -> Result<DeviceCodeResponse, String> {
        let resp = self
            .client
            .post(format!("{}/devicecode", self.authority()))
            .form(&[("client_id", self.client_id.as_str()), ("scope", SCOPES)])
            .send()
            .map_err(|e| e.to_string())?;
        let body = resp.text().map_err(|e| e.to_string())?;
        parse_device_code_response(&body).map_err(|e| e.to_string())
    }

    /// Blocks, polling at `device_code.interval` seconds, until the user
    /// completes sign-in or the code expires.
    pub fn poll_for_token(&self, device_code: &DeviceCodeResponse) -> Result<TokenResponse, String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(device_code.expires_in);
        loop {
            if std::time::Instant::now() >= deadline {
                return Err("device code expired before sign-in completed".to_string());
            }
            std::thread::sleep(Duration::from_secs(device_code.interval));

            let resp = self
                .client
                .post(format!("{}/token", self.authority()))
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", self.client_id.as_str()),
                    ("device_code", device_code.device_code.as_str()),
                ])
                .send()
                .map_err(|e| e.to_string())?;
            let status = resp.status().as_u16();
            let body = resp.text().map_err(|e| e.to_string())?;

            match parse_token_poll_response(status, &body) {
                TokenPollOutcome::Success(t) => return Ok(t),
                TokenPollOutcome::Pending => continue,
                TokenPollOutcome::Error(e) => return Err(e),
            }
        }
    }

    pub fn refresh_access_token(&self, refresh_token: &str) -> Result<TokenResponse, String> {
        let resp = self
            .client
            .post(format!("{}/token", self.authority()))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", self.client_id.as_str()),
                ("refresh_token", refresh_token),
                ("scope", SCOPES),
            ])
            .send()
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.text().map_err(|e| e.to_string())?;

        match parse_token_poll_response(status, &body) {
            TokenPollOutcome::Success(t) => Ok(t),
            TokenPollOutcome::Pending => Err("unexpected pending state on refresh".to_string()),
            TokenPollOutcome::Error(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_code_response() {
        let body = serde_json::json!({
            "device_code": "dc123",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://microsoft.com/devicelogin",
            "expires_in": 900,
            "interval": 5,
            "message": "To sign in, use a web browser to open the page https://microsoft.com/devicelogin and enter the code ABCD-EFGH to authenticate."
        })
        .to_string();
        let parsed = parse_device_code_response(&body).unwrap();
        assert_eq!(parsed.user_code, "ABCD-EFGH");
        assert_eq!(parsed.interval, 5);
    }

    #[test]
    fn successful_token_response_parses() {
        let body = serde_json::json!({
            "access_token": "eyabc",
            "refresh_token": "refresh-xyz",
            "expires_in": 3600,
            "token_type": "Bearer"
        })
        .to_string();
        let outcome = parse_token_poll_response(200, &body);
        assert_eq!(
            outcome,
            TokenPollOutcome::Success(TokenResponse {
                access_token: "eyabc".to_string(),
                refresh_token: Some("refresh-xyz".to_string()),
                expires_in: 3600,
            })
        );
    }

    #[test]
    fn authorization_pending_is_pending_not_error() {
        let body = serde_json::json!({
            "error": "authorization_pending",
            "error_description": "still waiting"
        })
        .to_string();
        assert_eq!(
            parse_token_poll_response(400, &body),
            TokenPollOutcome::Pending
        );
    }

    #[test]
    fn slow_down_is_pending() {
        let body = serde_json::json!({"error": "slow_down"}).to_string();
        assert_eq!(
            parse_token_poll_response(400, &body),
            TokenPollOutcome::Pending
        );
    }

    #[test]
    fn expired_token_error_is_terminal() {
        let body = serde_json::json!({
            "error": "expired_token",
            "error_description": "the device code expired"
        })
        .to_string();
        assert_eq!(
            parse_token_poll_response(400, &body),
            TokenPollOutcome::Error("expired_token".to_string())
        );
    }

    #[test]
    fn authorization_declined_is_terminal() {
        let body = serde_json::json!({"error": "authorization_declined"}).to_string();
        assert_eq!(
            parse_token_poll_response(400, &body),
            TokenPollOutcome::Error("authorization_declined".to_string())
        );
    }

    #[test]
    fn malformed_success_body_is_reported_as_error() {
        let outcome = parse_token_poll_response(200, "not json");
        assert!(matches!(outcome, TokenPollOutcome::Error(_)));
    }
}
