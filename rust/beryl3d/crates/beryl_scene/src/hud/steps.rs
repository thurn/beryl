//! The steps of the turn and the stops the player has set at them. Nothing until the game screen's
//! track builds it.

use crate::element::Element;
use crate::screens::ScreenIn;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn steps(_input: &ScreenIn) -> Option<Element> {
    None
}
