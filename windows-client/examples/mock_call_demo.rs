//! Verification-only: exercises the real `MockCallSource` -> `CallClassifier`
//! (unused here, mock emits pre-classified calls) -> `WorkDashClient::put_call`
//! path without touching Graph auth, so the incoming-call pipeline can be
//! demoed against a local server. Run with WORK_DASH_SERVER_URL /
//! WORK_DASH_API_KEY set and MOCK_INCOMING_CALLER for the caller name.

use work_dash_windows_client::push::WorkDashClient;
use work_dash_windows_client::teams::classify::IncomingCall;
use work_dash_windows_client::teams::listener_mock::MockCallSource;
use work_dash_windows_client::teams::source::CallSource;

fn main() {
    let server_url =
        std::env::var("WORK_DASH_SERVER_URL").expect("WORK_DASH_SERVER_URL must be set");
    let api_key = std::env::var("WORK_DASH_API_KEY").expect("WORK_DASH_API_KEY must be set");
    let caller = std::env::var("MOCK_INCOMING_CALLER").unwrap_or_else(|_| "Sarah Lee".to_string());

    let push_client = WorkDashClient::new(server_url, api_key);
    let source = MockCallSource::new(vec![IncomingCall { caller }]);
    let rx = source.start();

    let call = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("mock listener did not emit a call");

    push_client
        .put_call(&call.caller)
        .expect("push to server failed");
    println!("pushed incoming call for {}", call.caller);
}
