//! Optional host-side suggestions built from ordinary table actions.

pub mod combat;
pub mod nudges;
pub mod upkeep;

pub use crate::state::{Assist, Line, Proposal};
use crate::{Action, Seat, Table};

/// Whether a seat has enabled an assist.
#[must_use]
pub fn enabled(table: &Table, seat: Seat, assist: Assist) -> bool {
    table
        .assists
        .get(usize::from(seat.0))
        .is_some_and(|settings| settings.contains(&assist))
}

/// Builds one action from the lines the player has left checked.
#[must_use]
pub fn apply_proposal(proposal: &Proposal) -> Action {
    Action::Batch {
        actions: proposal
            .lines
            .iter()
            .filter(|line| line.on)
            .flat_map(|line| line.actions.iter().cloned())
            .collect(),
    }
}
