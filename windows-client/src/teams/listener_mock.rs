//! Dev/test `CallSource`: feeds a scripted sequence of `IncomingCall`s on a
//! background thread, standing in for the real Windows toast listener.
//! This is what makes the end-to-end sync path testable on Linux/WSL.

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use super::classify::IncomingCall;
use super::source::CallSource;

pub struct MockCallSource {
    scripted_calls: Vec<IncomingCall>,
    delay: Duration,
}

impl MockCallSource {
    pub fn new(scripted_calls: Vec<IncomingCall>) -> Self {
        MockCallSource {
            scripted_calls,
            delay: Duration::from_millis(0),
        }
    }

    /// Delay before emitting each scripted call — useful to simulate a call
    /// arriving mid-run rather than immediately at startup.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

impl CallSource for MockCallSource {
    fn start(self) -> Receiver<IncomingCall> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for call in self.scripted_calls {
                if !self.delay.is_zero() {
                    thread::sleep(self.delay);
                }
                if tx.send(call).is_err() {
                    break;
                }
            }
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn emits_scripted_calls_in_order() {
        let source = MockCallSource::new(vec![
            IncomingCall {
                caller: "Alice".to_string(),
            },
            IncomingCall {
                caller: "Bob".to_string(),
            },
        ]);
        let rx = source.start();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap().caller,
            "Alice"
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap().caller,
            "Bob"
        );
    }

    #[test]
    fn empty_script_closes_channel_with_no_events() {
        let source = MockCallSource::new(vec![]);
        let rx = source.start();
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    }
}
