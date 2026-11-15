//! The cards an effect revealed, shown until they are dismissed. Nothing until the game screen's
//! track builds it.

use crate::element::Element;
use crate::screens::ScreenIn;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn revealed(_input: &ScreenIn) -> Option<Element> {
    None
}
