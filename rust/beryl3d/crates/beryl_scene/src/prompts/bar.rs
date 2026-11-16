//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use std::borrow::Cow;

use beryl_protocol::prompt::PromptView;
use beryl_protocol::protocol::Status;

use crate::element::{Element, Press, row, text};
use crate::screens::ScreenIn;
use crate::tokens::{space, type_style};
use crate::widgets::{button, primary};

/// The prompt bar for `prompt`: its title, then its buttons in the order the session sent them,
/// the one the prompt puts forward drawn as the primary button.
/// A prompt with no title, such as a priority stop's, shows only its buttons, as the web
/// interface's prompt bar does (`ui/src/stage/PromptBar.tsx`): an empty text would still take
/// a gap beside them.
///
/// While the viewer controls the opponent's turn, a decision they make for the opponent says
/// "For opponent" before what it asks, and a priority says only that, as the web's `Asks` does
/// ([CR 724.5]): "While controlling another player, a player makes all choices and decisions
/// the controlled player is allowed to make or is told to make by the rules or by any objects."
///
/// [CR 724.5]: https://yawgatog.com/resources/magic-rules/#R7245
#[must_use]
pub fn prompt_bar(prompt: &PromptView, input: &ScreenIn) -> Element {
    let acting_for = input.view.is_some_and(|view| {
        matches!(&view.status, Status::Deciding { decision } if decision.acting_for.is_some())
    });
    let asks: Cow<str> = match (acting_for, prompt.title.is_empty()) {
        (false, _) => Cow::Borrowed(&prompt.title),
        (true, true) => Cow::Borrowed("For opponent"),
        (true, false) => Cow::Owned(format!("For opponent: {}", prompt.title)),
    };
    let title = (!asks.is_empty()).then(|| text(&asks).style(type_style::TITLE).grow(1.0));
    row()
        .name("prompt")
        .gap(space::M)
        .padding(space::L)
        .children(title)
        .children(prompt.buttons.iter().map(|button_view| {
            let widget = if button_view.primary { primary } else { button };
            widget(&button_view.label)
                .enabled(button_view.enabled)
                .on_press(Press::Button(button_view.id))
        }))
}
