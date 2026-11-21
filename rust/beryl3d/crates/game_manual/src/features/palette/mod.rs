//! The searchable list of manual table commands.

mod view;
pub use view::{Inputs, ViewKey};

use crate::manual::palette::Palette;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use std::convert::Infallible;

pub type State = Option<Palette>;

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Query(String),
    Highlight(bool),
    Choose(u16),
    CancelCapture,
    Close,
}

pub struct CommandList;

impl Feature for CommandList {
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
        let Some(palette) = state.as_mut() else {
            return false;
        };
        match message {
            Message::Query(query) => palette.set_query(query),
            Message::Highlight(down) => palette.move_highlight(*down),
            Message::Close => {
                *state = None;
                context.request(Message::Close);
            }
            Message::Choose(_) | Message::CancelCapture => {
                context.request(message.clone());
                return false;
            }
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs<'_>) -> ViewKey {
        view::key(inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(state, inputs)
    }
}
