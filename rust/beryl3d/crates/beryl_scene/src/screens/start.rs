//! The start screen, where a person picks an agent and starts a game. A region named for the screen
//! until the screens track builds it.

use super::ScreenIn;
use crate::element::{Element, column};

/// What the flat layer shows on the screen.
#[must_use]
pub fn start(_input: &ScreenIn) -> Element {
    column().name("start")
}
