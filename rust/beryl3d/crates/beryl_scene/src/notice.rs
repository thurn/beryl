//! What an `announce` step shows: a banner or a pill, from when it begins to when it goes.

use beryl_protocol::script::Notice;

use crate::verbs::Interval;

/// A notice shown over `over`, on the script's clock. `passes` says whether the table goes on
/// playing under it. `step` is the number of the step that wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeRun {
    pub notice: Notice,
    pub over: Interval,
    pub passes: bool,
    pub step: usize,
}
