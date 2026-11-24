//! Reporting failed hub writes without repeating misconfiguration errors.

use std::sync::atomic::{AtomicBool, Ordering};

use ignite_core::traits::Error;

use super::read_only;

/// Reports failed writes to the hub on standard error. A read-only listener is said once and
/// then not again until a write succeeds, so a wrong hub URL fills the log with one line and
/// not one per round.
#[derive(Debug, Default)]
pub struct FailureLog {
    said_read_only: AtomicBool,
}

impl FailureLog {
    /// Prints `ignite: <what>: <error>`, except that a read-only listener's error is printed
    /// once, until [`FailureLog::succeeded`].
    pub fn failed(&self, what: &str, error: &Error) {
        if self.worth_saying(error) {
            eprintln!("ignite: {what}: {error}");
        }
    }

    /// Whether `error` is to be printed: always, except for a read-only listener already said.
    pub(super) fn worth_saying(&self, error: &Error) -> bool {
        !(read_only(error) && self.said_read_only.swap(true, Ordering::Relaxed))
    }

    /// Notes that a write reached the hub, so a later read-only answer is reported again.
    pub fn succeeded(&self) {
        self.said_read_only.store(false, Ordering::Relaxed);
    }
}

/// The failures of the daemon's loops writing to the hub, which share one line.
pub static HUB_WRITES: FailureLog = FailureLog {
    said_read_only: AtomicBool::new(false),
};
