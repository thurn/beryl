//! The manual table’s prompt for a command’s number or text.

mod view;
pub use view::{Inputs, ViewKey};

use crate::manual::ask::Ask;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use std::convert::Infallible;

pub type State = Option<Ask>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Confirm,
    Step(bool),
    Suggestion(u8),
    Close,
}

pub struct AskPrompt;

impl Feature for AskPrompt {
    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Infallible;
    type Request = Message;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, Self::Request>,
    ) -> bool {
        if state.is_none() {
            return false;
        }
        if matches!(message, Message::Close) {
            *state = None;
        }
        context.request(*message);
        matches!(message, Message::Close)
    }

    fn view_key(state: &State, inputs: &Inputs<'_>) -> ViewKey {
        view::key(state, inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(state, inputs)
    }
}
