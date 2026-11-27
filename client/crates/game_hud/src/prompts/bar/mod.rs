//! The prompt bar and the buttons the game offers outside of a shape's controls.

mod heading;
mod question;
mod view;

use beryl_protocol::prompt::ButtonId;
use kit_scene::{Context, Feature};
use kit_ui::element::{Element, holder};

use super::feature::{Command, Details, Inputs, ViewKey};

pub(super) use view::Controls;
pub use view::{captioned, captioned_dock, dock_sized, draws_phase, title, turn_action};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Button(ButtonId),
    Undo,
}

pub struct PromptBar;

impl Feature for PromptBar {
    const SOURCE: &'static str = file!();

    type State = ();
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Command;
    type Request = std::convert::Infallible;

    fn update(
        (): &mut (),
        message: &Message,
        context: &mut Context<'_, Command, Self::Request>,
    ) -> bool {
        context.command(match *message {
            Message::Button(button) => {
                Command::Click(beryl_protocol::prompt::Click::Button { button })
            }
            Message::Undo => Command::Undo,
        });
        false
    }

    fn view_key((): &(), inputs: &Inputs<'_>) -> ViewKey {
        let view = inputs.hud.view;
        let details = Details::Bar {
            acting_for: view.is_some_and(view::acting_for),
            opponent_acts: view.is_some_and(view::opponent_acts),
            shared: view.map(|view| view.shared.is_some()),
            turn: view.map(|view| (view.turn.active, view.turn.step)),
            in_hand: (view.zip(inputs.hud.prompt))
                .is_some_and(|(view, prompt)| heading::picks_in_hand(prompt, view)),
        };
        ViewKey::new(inputs, details)
    }

    fn view((): &(), inputs: Inputs<'_>) -> Element {
        let hud = &inputs.hud;
        if let Some(prompt) = hud.prompt {
            view::bar(prompt, hud, super::number::stepper(prompt, hud))
        } else {
            view::waiting(hud).unwrap_or_else(holder)
        }
    }
}

/// The bar has a view while the player can answer or the opponent acts.
#[must_use]
pub fn appears(input: &crate::hud::HudIn<'_>) -> bool {
    input.prompt.is_some() || input.view.is_some_and(view::opponent_acts)
}
