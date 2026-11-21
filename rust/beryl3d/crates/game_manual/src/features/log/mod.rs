//! The manual table’s foldable log and its history actions.

mod view;
pub use view::{Inputs, Placement, ViewKey};

use crate::manual::log;
use beryl_protocol::protocol::TableAction;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use std::convert::Infallible;

#[derive(Debug, Default)]
pub struct State {
    pub open: bool,
    pub blocked: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Undo(u32),
    Rewind(u32),
    Toggle,
    ClearBlocked,
    Blocked(Vec<u32>),
}

pub struct TableLog;

impl Feature for TableLog {
    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = TableAction;
    type Request = Infallible;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, Self::Request>,
    ) -> bool {
        match message {
            Message::Undo(entry) => context.command(log::undo(*entry)),
            Message::Rewind(entry) => context.command(log::rewind(*entry)),
            Message::Toggle => {
                state.open = !state.open;
                return true;
            }
            Message::ClearBlocked => {
                let changed = !state.blocked.is_empty();
                state.blocked.clear();
                return changed;
            }
            Message::Blocked(entries) if !entries.is_empty() => {
                state.blocked.clone_from(entries);
                state.open = true;
                return true;
            }
            Message::Blocked(_) => {}
        }
        false
    }

    fn view_key(_: &State, inputs: &Inputs<'_>) -> ViewKey {
        view::key(inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(state, inputs)
    }
}
