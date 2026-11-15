//! The viewer, which shows the cards of a pile such as a graveyard. Nothing until the game screen's
//! track builds it.

use crate::element::Element;
use crate::screens::ScreenIn;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn viewer(_input: &ScreenIn) -> Option<Element> {
    None
}
