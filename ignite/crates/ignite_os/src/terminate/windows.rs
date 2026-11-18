use std::io;

use super::LastStep;

/// The Task Scheduler ends the process without a signal a handler could catch, so a stop ends it
/// as it did before and the step never runs.
#[allow(clippy::unnecessary_wraps)]
pub fn on_terminate(last: LastStep) -> io::Result<()> {
    drop(last);
    Ok(())
}
