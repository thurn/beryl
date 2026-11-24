//! The split prompt and the steps it makes on the table.

mod view;

use super::{Shown, Steps};
use crate::prompts::tray;

/// Makes what the table shows for the prompt `shown` replace what it showed for `was`, as one
/// beat (`tray::own`): the marks the session sends on the cards, and the two piles held up in
/// the tray, sorted again after each reply.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    tray::own(shown, was, tray::marks, steps);
}

prompt_feature!(Split, view::view, own);
