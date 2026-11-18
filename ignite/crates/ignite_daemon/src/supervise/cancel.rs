//! Ending a command early: when its caller leaves, or when the work it belongs to is asked to
//! stop (see `crate::steps`).

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use ignite_core::protocol::Usage;
use ignite_core::traits::{CommandSpec, RunResult};

use super::Run;

/// How often a command whose caller may leave is checked for the caller's departure.
const CANCEL_POLL: Duration = Duration::from_secs(1);
/// Seconds a command has to exit after it is asked to, because its caller left, before its
/// group is killed.
const CANCEL_GRACE: Duration = Duration::from_secs(10);

/// Ends `run`'s command once `cancelled` is true: first with `SIGTERM` (or the job object's
/// end), then, if it is still running [`CANCEL_GRACE`] later, with `SIGKILL`. Returns when
/// `finished` is disconnected, which happens when the command has finished.
pub(super) fn end_when_cancelled(
    run: &Run,
    cancelled: &(dyn Fn() -> bool + Sync),
    finished: &mpsc::Receiver<()>,
) {
    loop {
        match finished.recv_timeout(CANCEL_POLL) {
            Err(mpsc::RecvTimeoutError::Timeout) if cancelled() => {
                let _ = run.end();
                if matches!(
                    finished.recv_timeout(CANCEL_GRACE),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let _ = run.kill();
                }
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => return,
        }
    }
}

/// The result of a command that never started because its work was asked to stop, or whose
/// caller left while it waited for cores.
pub(super) fn stopped(spec: &CommandSpec) -> RunResult {
    eprintln!("ignite: not starting {:?}: its work was stopped", spec.args);
    RunResult {
        exit: None,
        output: PathBuf::new(),
        usage: Usage::default(),
        hung: false,
    }
}

/// The result of a command whose supervisor could not be started.
pub(super) fn not_started(spec: &CommandSpec, error: &io::Error) -> RunResult {
    eprintln!(
        "ignite: cannot start a supervisor for {:?}: {error}",
        spec.args
    );
    RunResult {
        exit: None,
        output: PathBuf::new(),
        usage: Usage::default(),
        hung: false,
    }
}
