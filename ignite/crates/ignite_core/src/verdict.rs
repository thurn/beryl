//! Verdicts: what a check's runs add up to, counting the immediate rerun of a failure and the
//! retries of an incomplete run.
//!
//! Stub. The ticket Thrh4.6 (Verdicts and exploration shares) owns this file and replaces the
//! `todo!()` bodies.
#![allow(unused_variables)]

use crate::protocol::Outcome;

/// What to do after a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// The verdict is final.
    Done(Outcome),
    /// Run the check again at once.
    Rerun,
    /// Incomplete too often: the hub files a ticket and anything needing the check waits.
    GiveUp,
}

/// The next step after `runs`, the outcomes so far in order.
pub fn next(runs: &[Outcome]) -> Next {
    todo!("Thrh4.6 (Verdicts and exploration shares)")
}
