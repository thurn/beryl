//! The prompt shape `Catalog`: one entry of a list. Until the prompts track builds them, they show
//! the prompt bar and draw nothing of their own on the table.

use beryl_protocol::prompt::PromptView;
use beryl_protocol::script::OwnTimings;

use super::prompt_bar;
use crate::element::Element;
use crate::light::Lit;
use crate::screens::ScreenIn;

/// What the flat layer shows for `prompt`.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &ScreenIn) -> Option<Element> {
    Some(prompt_bar(prompt, input))
}

/// The marks and arrows the client draws itself for `prompt`: none yet.
pub(super) fn own(
    _prompt: &PromptView,
    _was: Option<&PromptView>,
    _lit: &mut Lit,
    _timings: &OwnTimings,
    _now: u64,
) {
}
