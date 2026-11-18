//! One last step before the process ends because it was asked to stop.
//!
//! On macOS that request is `SIGTERM`, which launchd sends for `launchctl kickstart -k`,
//! `bootout` and an unload. Without a handler the process ends at once; [`on_terminate`] runs
//! the step on a thread of its own and then ends the process with the same signal, so whoever
//! sent it sees the same exit as before.
//!
//! On Windows the Task Scheduler stops a task by ending its process outright, which no handler
//! can see, so [`on_terminate`] does nothing there and a stop ends the process as it always has.

use std::io;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// The step to run before the process ends.
pub type LastStep = Box<dyn FnOnce() + Send + 'static>;

/// Runs `last` when the process is asked to stop, then lets it end as it would have without a
/// handler. Call it at most once per process.
pub fn on_terminate(last: LastStep) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    return macos::on_terminate(last);
    #[cfg(windows)]
    return windows::on_terminate(last);
}

#[cfg(all(test, target_os = "macos"))]
#[path = "terminate/tests.rs"]
mod tests;
