//! How other threads hand inputs to a host: session updates, the session's replies and finished
//! assets.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use beryl_scene::Input;

/// Hands inputs to the host from other threads. Each one also wakes the host's event loop, so
/// a still table responds at once.
#[derive(Clone)]
pub struct Inbox {
    sender: Sender<Input>,
    /// Wakes the event loop; false once it has ended.
    wake: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl Inbox {
    /// An inbox that sends inputs down `sender` and then calls `wake`.
    pub fn new(sender: Sender<Input>, wake: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self {
            sender,
            wake: Arc::new(wake),
        }
    }

    /// Sends `input` to the client. False once the host has ended.
    pub fn send(&self, input: Input) -> bool {
        self.sender.send(input).is_ok() && (self.wake)()
    }
}
