//! Game over, shown on the game screen once the game has ended. Nothing until the screens track
//! builds it.

use super::ScreenIn;
use crate::element::Element;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn game_over(_input: &ScreenIn) -> Option<Element> {
    None
}
