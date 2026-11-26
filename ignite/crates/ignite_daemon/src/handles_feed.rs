//! Step subscription feed for a handle while a client waits on it.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use ignite_core::ids::Handle;

use crate::steps::{Activity, Steps};

/// Each step of the operation running under a handle, as it starts, read from whichever activity
/// the steps registry lists for the handle: the one of a verb on a thread of the daemon, or the
/// one the daemon keeps for a verb process (see [`crate::host`]). The activity is looked up
/// again once it ends, as when a verb whose process died is run again by the daemon.
pub(crate) struct StepFeed {
    steps: Arc<Steps>,
    handle: Handle,
    current: Option<(Arc<Activity>, Receiver<String>)>,
}

impl StepFeed {
    pub(crate) fn new(steps: Arc<Steps>, handle: Handle) -> StepFeed {
        StepFeed {
            steps,
            handle,
            current: None,
        }
    }

    /// The steps heard within `timeout`: it returns at the first one, with any that came with it.
    /// The first call that finds the activity starts with its step now, if it has one, so a
    /// client that comes in the middle, as after a restart, sees where the work is at once.
    pub(crate) fn next(&mut self, timeout: Duration) -> Vec<String> {
        if self
            .current
            .as_ref()
            .is_none_or(|(activity, _)| activity.is_finished())
        {
            self.current = self.steps.by_handle(&self.handle).map(|activity| {
                let heard = activity.subscribe();
                (activity, heard)
            });
            if let Some(step) = self.current.as_ref().and_then(|(a, _)| a.view().step) {
                return vec![step];
            }
        }
        let Some((_, heard)) = &self.current else {
            std::thread::sleep(timeout);
            return Vec::new();
        };
        match heard.recv_timeout(timeout) {
            Ok(line) => {
                let mut lines = vec![line];
                lines.extend(heard.try_iter());
                lines
            }
            Err(RecvTimeoutError::Timeout) => Vec::new(),
            Err(RecvTimeoutError::Disconnected) => {
                self.current = None;
                Vec::new()
            }
        }
    }
}
