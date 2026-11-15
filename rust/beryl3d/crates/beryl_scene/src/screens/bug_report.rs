//! The bug report sheet. A region named for it until the screens track builds it.

use super::ScreenIn;
use crate::element::{Element, column};

/// What the flat layer shows for it.
#[must_use]
pub fn bug_report(_input: &ScreenIn) -> Element {
    column().name("bug report")
}
