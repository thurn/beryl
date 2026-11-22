//! The tray for ordering cards put onto a library.

mod view;
pub use view::{Inputs, ViewKey};

use crate::manual::order::LibraryOrder;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use std::convert::Infallible;

pub type State = Option<LibraryOrder>;

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Reset,
    Open(LibraryOrder),
    Card(u16),
    Nudge(bool),
    Done,
    Close,
}

pub enum Request {
    Done(LibraryOrder),
    ClearFocus,
}

pub struct OrderTray;

impl Feature for OrderTray {
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
            Message::Reset => return state.take().is_some(),
            Message::Open(order) => *state = Some(order.clone()),
            Message::Card(at) => {
                let Some(order) = state else { return false };
                order.shift(usize::from(*at), 0);
            }
            Message::Nudge(later) => {
                let Some(order) = state else { return false };
                order.nudge(*later);
            }
            Message::Done => {
                let Some(order) = state.take() else {
                    return false;
                };
                context.request(Request::Done(order));
            }
            Message::Close => {
                if state.take().is_none() {
                    return false;
                }
                context.request(Request::ClearFocus);
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
