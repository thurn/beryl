//! The manual table’s prompt for a command’s number or text.

mod view;
pub use view::{Inputs, ViewKey};

use crate::manual::ask::Ask;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use std::convert::Infallible;

pub type State = Option<Ask>;

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Open(Ask),
    Reset,
    Confirm,
    Step(bool),
    Suggestion(u8),
    Close,
}

pub enum Request {
    Confirm,
    Step(bool),
    Suggestion(u8),
    ClearFocus,
}

pub struct AskPrompt;

impl Feature for AskPrompt {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Infallible;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, Self::Request>,
    ) -> bool {
        match message {
            Message::Open(ask) => {
                *state = Some(ask.clone());
                return true;
            }
            Message::Reset => return state.take().is_some(),
            _ if state.is_none() => return false,
            Message::Confirm => context.request(Request::Confirm),
            Message::Step(up) => context.request(Request::Step(*up)),
            Message::Suggestion(at) => context.request(Request::Suggestion(*at)),
            Message::Close => {
                *state = None;
                context.request(Request::ClearFocus);
                return true;
            }
        }
        false
    }

    fn view_key(state: &State, inputs: &Inputs<'_>) -> ViewKey {
        view::key(state, inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(state, inputs)
    }
}
