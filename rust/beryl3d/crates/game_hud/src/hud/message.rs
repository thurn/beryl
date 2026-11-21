//! Messages sent by the game screen's controls.

use beryl_protocol::protocol::Step;

use crate::inspect::Viewed;

/// An interaction with the game screen or its overlays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamePress {
    Stop { step: Step, theirs: bool },
    View(Viewed),
    DismissRevealed,
    DismissViewer,
}
