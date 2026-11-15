//! Each player's life total, name and details, beside their side of the table. Nothing until the
//! game screen's track builds it.

use crate::element::Element;
use crate::screens::ScreenIn;

/// What the game screen shows for it, if anything.
#[must_use]
pub fn players(_input: &ScreenIn) -> Option<Element> {
    None
}
