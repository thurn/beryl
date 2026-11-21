//! The game log's presentation state and messages.

mod history;
mod lines;
mod view;

pub use history::{LogEntry, Logged};
pub use lines::{LogKind, LogLine, Names, counter_name, learn_names, listed, log_lines};

use std::convert::Infallible;

use game_table::profiles::Profile;
use kit_base::data::profiles::Piece;
use kit_base::geometry::Rect;
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
    Column { toggle: Rect, panel: Rect },
    Sheet(Rect),
}

impl Layout {
    /// The log presentation mounted on the game screen, if this profile has one.
    #[must_use]
    pub fn for_profile(profile: Profile, open: bool) -> Option<Self> {
        profile
            .data()
            .hud
            .overlays
            .iter()
            .find_map(|piece| match piece {
                Piece::LogColumn => Some(Self::Column {
                    toggle: profile.slot("log.toggle")?.rect,
                    panel: profile.slot("log")?.rect,
                }),
                Piece::LogSheet if open => Some(Self::Sheet(profile.slot("log")?.rect)),
                _ => None,
            })
    }
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
            Layout::Column { toggle, panel } => {
                view::log_column(state.open, entries, toggle, panel)
            }
            Layout::Sheet(area) => view::sheet(entries, area),
        }
    }
}
