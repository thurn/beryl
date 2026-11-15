//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use beryl_session::prompt::PromptView;

use super::{Element, Press, button, row, text};
use crate::tokens::{space, type_style};

/// The prompt bar for `prompt`: its title, then its buttons in the order the session sent them.
#[must_use]
pub fn prompt_bar(prompt: &PromptView) -> Element {
    row()
        .name("prompt")
        .gap(space::M)
        .padding(space::L)
        .child(text(&prompt.title).style(type_style::TITLE).grow(1.0))
        .children(prompt.buttons.iter().map(|button_view| {
            button(&button_view.label)
                .enabled(button_view.enabled)
                .on_press(Press::Button(button_view.id))
        }))
}
