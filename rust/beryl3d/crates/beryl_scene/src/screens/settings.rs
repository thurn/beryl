//! The settings sheet, with the sound levels and the stops. A region named for it until the screens
//! track builds it.

use super::ScreenIn;
use crate::element::{Element, column};

/// What the flat layer shows for it.
#[must_use]
pub fn settings(_input: &ScreenIn) -> Element {
    column().name("settings")
}
