//! Prompts as the session sends them: a [`PromptView`] is a prompt as its seat sees it, and a
//! [`ClickRequest`] is what the UI sends back (`beryl_session::prompt` holds a prompt's state
//! and words it). A [`PromptView`] prints as a few lines of text (`text.rs`), for the
//! hand-written expectations of tests.

mod text;
mod view;

pub use crate::protocol::ObjectId;
/// What a prompt's pick is for, which [`PromptView::purpose`] names.
pub use beryl_card::Purpose;
pub use view::{
    ButtonId, Click, ClickReply, ClickRequest, Lies, Marked, Pair, PartName, Pile, PlaceName,
    PromptButton, PromptView, ShapeBody, Share, TapPreview, Words, purpose_decl, shape_name,
};
