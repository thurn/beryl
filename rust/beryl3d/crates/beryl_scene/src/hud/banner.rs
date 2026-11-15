//! The banner that says whose turn begins. Nothing until the game screen's track builds it.

use crate::element::Element;
use crate::screens::ScreenIn;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn banner(_input: &ScreenIn) -> Option<Element> {
    None
}
