//! The prompt shape `Declare`: objects each aimed at something, such as attackers and blockers.
//! Until the prompts track builds them, they show the prompt bar and draw nothing of their own on
//! the table.

use beryl_protocol::prompt::PromptView;

use super::{Shown, Steps, prompt_bar};
use crate::element::Element;
use crate::screens::ScreenIn;

/// What the flat layer shows for `prompt`.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &ScreenIn) -> Option<Element> {
    Some(prompt_bar(prompt, input))
}

/// The marks and arrows the client draws itself for the prompt `shown`, which was `was`: none
/// yet.
pub(super) fn own(_shown: Option<Shown>, _was: Option<Shown>, _steps: &mut Steps) {}
