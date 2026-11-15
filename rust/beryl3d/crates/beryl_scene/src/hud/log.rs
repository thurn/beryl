//! The game log, a list of what has happened. Nothing until the game screen's track builds it.

use crate::element::Element;
use crate::screens::ScreenIn;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn log(_input: &ScreenIn) -> Option<Element> {
    None
}
