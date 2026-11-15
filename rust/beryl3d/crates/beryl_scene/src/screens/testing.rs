//! The testing screen. A region named for the screen until the screens track builds it.

use super::ScreenIn;
use crate::element::{Element, column};

/// What the flat layer shows on the screen.
#[must_use]
pub fn testing(_input: &ScreenIn) -> Element {
    column().name("testing")
}
