//! Thin HTTP client pushing calendar/unread-count/call data to
//! `work-dash-server`. Mirrors the existing Ratatui client's pattern:
//! blocking `reqwest`, bearer auth, base URL from config
//! (`client/src/net.rs`).

use crate::models::{CalendarPutBody, PutCallBody, SetUnreadCount};

#[derive(Debug)]
pub struct PushError(String);

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PushError {}

impl From<reqwest::Error> for PushError {
    fn from(e: reqwest::Error) -> Self {
        PushError(e.to_string())
    }
}

#[derive(Clone)]
pub struct WorkDashClient {
    base_url: String,
    api_key: String,
    client: reqwest::blocking::Client,
}

impl WorkDashClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        WorkDashClient {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn put_calendar(&self, body: &CalendarPutBody) -> Result<(), PushError> {
        self.client
            .put(format!("{}/api/calendar", self.base_url))
            .bearer_auth(&self.api_key)
            .json(body)
            .send()?
            .error_for_status()?;
        Ok(())
    }

    /// Pushes the current absolute unread count from a Graph poll-and-diff
    /// pass — see `graph::chats`. Never a delta.
    pub fn set_unread_count(&self, count: i64) -> Result<(), PushError> {
        self.client
            .put(format!("{}/api/teams", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&SetUnreadCount { count })
            .send()?
            .error_for_status()?;
        Ok(())
    }

    /// Raises the board's incoming-call banner. The server holds this as an
    /// ephemeral singleton — no history, no retention.
    pub fn put_call(&self, caller: &str) -> Result<(), PushError> {
        self.client
            .put(format!("{}/api/call", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&PutCallBody {
                caller: caller.to_string(),
            })
            .send()?
            .error_for_status()?;
        Ok(())
    }

    /// Clears the call banner server-side (e.g. once the ring stops).
    pub fn clear_call(&self) -> Result<(), PushError> {
        self.client
            .delete(format!("{}/api/call", self.base_url))
            .bearer_auth(&self.api_key)
            .send()?
            .error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CalendarEventIn;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// Minimal single-request HTTP stub: accepts one connection, parses the
    /// request line, headers, and body, hands them to `assertion`, replies
    /// with `status_line`. Good enough to verify method/path/headers/body
    /// without pulling in a full test server dependency.
    fn serve_one(
        status_line: &'static str,
        assertion: impl FnOnce(&str, &str, &[(String, String)], &str) + Send + 'static,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());

            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut parts = request_line.trim().splitn(3, ' ');
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();

            let mut headers = Vec::new();
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    let k = k.trim().to_string();
                    let v = v.trim().to_string();
                    if k.eq_ignore_ascii_case("content-length") {
                        content_length = v.parse().unwrap_or(0);
                    }
                    headers.push((k, v));
                }
            }

            let mut body_buf = vec![0u8; content_length];
            reader.read_exact(&mut body_buf).unwrap();
            let body = String::from_utf8(body_buf).unwrap();

            assertion(&method, &path, &headers, &body);

            let mut stream = stream;
            stream.write_all(status_line.as_bytes()).unwrap();
        });

        format!("http://{addr}")
    }

    #[test]
    fn put_calendar_sends_expected_method_auth_and_body() {
        let url = serve_one(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}",
            |method, path, headers, body| {
                assert_eq!(method, "PUT");
                assert_eq!(path, "/api/calendar");
                assert!(headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                        && v == "Bearer test-key"));
                let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!(parsed["events"][0]["external_id"], "abc123");
                assert_eq!(parsed["events"][0]["title"], "Standup");
                assert_eq!(parsed["events"][0]["is_cancelled"], false);
            },
        );

        let client = WorkDashClient::new(url, "test-key");
        let body = CalendarPutBody {
            events: vec![CalendarEventIn {
                external_id: "abc123".to_string(),
                title: "Standup".to_string(),
                start: "2026-07-06T09:00:00Z".to_string(),
                end: "2026-07-06T09:30:00Z".to_string(),
                place: None,
                is_cancelled: false,
            }],
            range_start: "2026-07-06T00:00:00Z".to_string(),
            range_end: "2026-07-07T00:00:00Z".to_string(),
        };
        client.put_calendar(&body).unwrap();
    }

    #[test]
    fn set_unread_count_sends_absolute_count() {
        let url = serve_one(
            "HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\n{}",
            |method, path, headers, body| {
                assert_eq!(method, "PUT");
                assert_eq!(path, "/api/teams");
                assert!(headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("authorization")
                        && v == "Bearer test-key"));
                let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!(parsed["count"], 4);
            },
        );

        let client = WorkDashClient::new(url, "test-key");
        client.set_unread_count(4).unwrap();
    }

    #[test]
    fn put_call_sends_caller_name() {
        let url = serve_one(
            "HTTP/1.1 204 No Content\r\n\r\n",
            |method, path, _headers, body| {
                assert_eq!(method, "PUT");
                assert_eq!(path, "/api/call");
                let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!(parsed["caller"], "Sarah Lee");
            },
        );

        let client = WorkDashClient::new(url, "test-key");
        client.put_call("Sarah Lee").unwrap();
    }

    #[test]
    fn clear_call_sends_delete_with_no_body() {
        let url = serve_one("HTTP/1.1 204 No Content\r\n\r\n", |method, path, _, body| {
            assert_eq!(method, "DELETE");
            assert_eq!(path, "/api/call");
            assert_eq!(body, "");
        });

        let client = WorkDashClient::new(url, "test-key");
        client.clear_call().unwrap();
    }

    #[test]
    fn trailing_slash_in_base_url_is_stripped() {
        let client = WorkDashClient::new("http://localhost:8080/", "k");
        assert_eq!(client.base_url, "http://localhost:8080");
    }

    #[test]
    fn error_status_surfaces_as_err() {
        let url = serve_one(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 20\r\n\r\n{\"error\":\"nope\"}",
            |_, _, _, _| {},
        );
        let client = WorkDashClient::new(url, "bad-key");
        assert!(client.put_call("x").is_err());
    }
}
