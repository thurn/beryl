//! The game log's presentation state and messages.

mod history;
mod lines;
mod view;

pub use history::{LogEntry, Logged};
pub use lines::{LogKind, LogLine, Names, counter_name, learn_names, listed, log_lines};

use std::convert::Infallible;

use game_table::profiles::TableBands;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;

/// The name of the list, which the client scrolls to its foot.
pub const LIST: &str = "Game log";

pub struct GameLog;

#[derive(Debug, Default)]
pub struct State {
    pub open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Toggle,
}

pub enum Request {
    /// The phone's settings sheet may be over the game screen when the log opens.
    DismissSheet,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    Sidebar,
    Sheet(TableBands),
}

pub struct Inputs<'a> {
    pub layout: Layout,
    pub history: &'a Logged,
}

impl Feature for GameLog {
    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = (Layout, Option<(u32, u32)>);
    type Command = Infallible;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Infallible, Request>,
    ) -> bool {
        match message {
            Message::Toggle => {
                state.open = !state.open;
                context.request(Request::DismissSheet);
                true
            }
        }
    }

    fn view_key(state: &State, inputs: &Inputs<'_>) -> Self::ViewKey {
        let entries = inputs.history.entries();
        // The keys between these endpoints can only change if the last key
        // changes: undo truncates a suffix, and every new line gets a new key.
        let lines = if state.open {
            entries
                .first()
                .zip(entries.last())
                .map(|(first, last)| (first.key, last.key))
        } else {
            None
        };
        (inputs.layout, lines)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        let entries = inputs.history.entries();
        match inputs.layout {
            Layout::Sidebar => view::desktop(state.open, entries),
            Layout::Sheet(bands) => view::phone(entries, &bands),
        }
    }
}
