//! Stub. The ticket "CLI mode: read a running turn and apply what follows its end" owns this
//! file and replaces the `todo!()` body.
#![allow(unused_variables)]

use ignite_core::cli::{Action, Slot};
use ignite_core::traits::Result;

/// Carries out what `decide` chose after `slot`'s turn ended: free the slot, start the next
/// turn, wait for a verb, back off, park, or release the ticket.
pub fn apply(slot: &Slot, action: &Action) -> Result<()> {
    todo!()
}
