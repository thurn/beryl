//! The inspector, which shows one card at full size with its details. Nothing until the game
//! screen's track builds it.

use crate::element::Element;
use crate::screens::ScreenIn;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn inspector(_input: &ScreenIn) -> Option<Element> {
    None
}
