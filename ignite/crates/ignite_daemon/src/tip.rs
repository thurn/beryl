//! Tip verification: once per tip, run the checks the gate left out and move the release branch
//! forward; on a failure, find the culprit and report a regression.
//!
//! Stub. The ticket Thrh4.30 (Tip verification and finding the culprit) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::ids::{CheckName, Commit};

use crate::server::Context;

/// How one tip's verification ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TipOutcome {
    Verified,
    Failed { checks: Vec<CheckName> },
}

pub fn verify(ctx: &Context, tip: &Commit) -> TipOutcome {
    todo!("Thrh4.30 (Tip verification and finding the culprit)")
}

/// The first commit between `good` and `bad` at which `check` fails.
pub fn find_culprit(
    ctx: &Context,
    check: &CheckName,
    good: &Commit,
    bad: &Commit,
) -> Option<Commit> {
    todo!("Thrh4.30 (Tip verification and finding the culprit)")
}
