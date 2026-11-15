//! The card gallery. A region named for the screen until the screens track builds it.

use super::ScreenIn;
use crate::element::{Element, column};

/// What the flat layer shows on the screen.
#[must_use]
pub fn gallery(_input: &ScreenIn) -> Element {
    column().name("gallery")
}
