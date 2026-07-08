//! `CallSource` abstracts "something that emits incoming Teams calls" so
//! the sync loop is testable on any platform: real runs use
//! `listener_win::ToastCallSource` (Windows only), tests/dev use
//! `listener_mock::MockCallSource`.

use std::sync::mpsc::Receiver;

use super::classify::IncomingCall;

pub trait CallSource {
    /// Starts listening in the background; returns a channel that yields
    /// each detected incoming call as it happens.
    fn start(self) -> Receiver<IncomingCall>;
}
