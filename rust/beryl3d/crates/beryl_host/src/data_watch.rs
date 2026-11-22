//! Watches the app's runtime tuning away from the frame thread.

use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use app_client::Input;
use kit_base::data::Data;

use crate::Inbox;

/// A runtime tuning source and its last stable read, including failed reads.
pub struct DataWatch<Snapshot> {
    read: Box<dyn FnMut() -> Result<Snapshot, String> + Send>,
    parse: fn(&Snapshot) -> Result<Data, String>,
    seen: Option<Result<Snapshot, String>>,
}

impl<Snapshot: PartialEq + Send + 'static> DataWatch<Snapshot> {
    /// Watches the complete input sets returned by `read`, validating them with `parse`.
    pub fn new(
        read: impl FnMut() -> Result<Snapshot, String> + Send + 'static,
        parse: fn(&Snapshot) -> Result<Data, String>,
    ) -> Self {
        Self {
            read: Box::new(read),
            parse,
            seen: None,
        }
    }

    /// Returns a new set or error once two consecutive reads agree. A save during the first
    /// read can mix files from before and after the edit, so validate only the captured set
    /// that agrees with the second read. Leave a changing set for the next poll.
    pub fn poll(&mut self) -> Option<Result<Arc<Data>, String>> {
        let candidate = (self.read)();
        if self.seen.as_ref() == Some(&candidate) || (self.read)() != candidate {
            return None;
        }
        let data = candidate
            .as_ref()
            .map_err(Clone::clone)
            .and_then(self.parse)
            .map(Arc::new);
        self.seen = Some(candidate);
        Some(data)
    }

    /// Polls for edits on a utility thread. Sending via the inbox wakes the host even when
    /// playback is paused.
    ///
    /// # Errors
    ///
    /// When the operating system cannot start the thread.
    pub fn start(mut self, inbox: Inbox) -> io::Result<WatchingData> {
        let (stop, stopped) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("beryl3d-data".to_owned())
            .spawn(move || {
                crate::utility_thread();
                while stopped.recv_timeout(Duration::from_millis(250))
                    == Err(RecvTimeoutError::Timeout)
                {
                    if let Some(data) = self.poll() {
                        let input = match data {
                            Ok(data) => Input::Data(data),
                            Err(error) => Input::DataError(error),
                        };
                        if !inbox.send(input) {
                            break;
                        }
                    }
                }
            })?;
        Ok(WatchingData {
            stop,
            thread: Some(thread),
        })
    }
}

/// The lifetime of a running data watcher. Dropping it wakes and joins its thread.
pub struct WatchingData {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for WatchingData {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
