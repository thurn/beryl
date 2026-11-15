//! The opening hand and the mulligan, shown on the game screen before the first turn. Nothing until
//! the screens track builds it.

use super::ScreenIn;
use crate::element::Element;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn opening(_input: &ScreenIn) -> Option<Element> {
    None
}
