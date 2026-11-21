//! Turn changes compiled from scripts.

use beryl_protocol::script::Turn;

/// A step of the turn that a script's `set` step begins at `at`, on the script's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnChange {
    pub turn: Turn,
    pub at: u64,
    /// The number of the step that wrote it.
    pub step: usize,
}
