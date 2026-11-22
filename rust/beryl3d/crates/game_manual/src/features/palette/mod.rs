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
    Open(Palette),
    Reset,
    Capture(bool),
    Query(String),
    Highlight(bool),
    Choose(u16),
    CancelCapture,
    Close,
}

pub enum Request {
    Choose(u16),
    CancelCapture,
    ClearFocus,
}

pub struct CommandList;

impl Feature for CommandList {
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
        let palette = match message {
            Message::Open(palette) => {
                *state = Some(palette.clone());
                return true;
            }
            Message::Reset => return state.take().is_some(),
            _ => match state.as_mut() {
                Some(palette) => palette,
                None => return false,
            },
        };
        match message {
            Message::Open(_) | Message::Reset => unreachable!("handled above"),
            Message::Capture(capturing) => palette.capturing = *capturing,
            Message::Query(query) => palette.set_query(query),
            Message::Highlight(down) => palette.move_highlight(*down),
            Message::Close => {
                *state = None;
                context.request(Request::ClearFocus);
            }
            Message::Choose(at) => {
                context.request(Request::Choose(*at));
                return false;
            }
            Message::CancelCapture => {
                context.request(Request::CancelCapture);
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
