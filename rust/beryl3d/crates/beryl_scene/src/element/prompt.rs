//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use beryl_protocol::prompt::PromptView;

use super::{Element, Press, button, row, text};
use crate::tokens::{space, type_style};

/// The prompt bar for `prompt`: its title, then its buttons in the order the session sent them.
/// A prompt with no title, such as a priority stop's, shows only its buttons, as the web
/// interface's prompt bar does (`ui/src/stage/PromptBar.tsx`): an empty text would still take
/// a gap beside them.
#[must_use]
pub fn prompt_bar(prompt: &PromptView) -> Element {
    let title =
        (!prompt.title.is_empty()).then(|| text(&prompt.title).style(type_style::TITLE).grow(1.0));
    row()
        .name("prompt")
        .gap(space::M)
        .padding(space::L)
        .children(title)
        .children(prompt.buttons.iter().map(|button_view| {
            button(&button_view.label)
                .enabled(button_view.enabled)
                .on_press(Press::Button(button_view.id))
        }))
}
