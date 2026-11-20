//! Stub. The ticket "CLI mode: the pure function that decides what follows a turn" owns this
//! file and replaces the `todo!()` body.
#![allow(unused_variables)]

use super::{Action, Facts, Slot, TurnEnd};

/// What follows the end of `slot`'s latest turn, by the rules of "How a turn ends" in the
/// design document. No I/O and no clock but `facts.now`. An end or a combination the rules do
/// not name returns [`Action::Park`], never a release or a start.
pub fn decide(slot: &Slot, end: &TurnEnd, facts: &Facts) -> Action {
    todo!()
}
