//! The recordings browser and the seat to replay from.

mod view;

use std::sync::Arc;

use beryl_protocol::protocol::Side;
use game_table::table::Window;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use kit_ui::widgets::FieldState;
use kit_ui::widgets::screen::ScreenColumn;

pub const FILTER: &str = "filter";

pub struct TestingScreen;

#[derive(Debug)]
pub struct State {
    pub recordings: Vec<String>,
    stories: Arc<[String]>,
    seat: Side,
    filter: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            recordings: Vec::new(),
            stories: Arc::default(),
            seat: Side::You,
            filter: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Recordings(Vec<String>),
    Stories(Arc<[String]>),
    Story(String),
    Replay { recording: u16 },
    Seat(Side),
    Filter(String),
}

pub enum Request {
    Replay { name: String, seat: Side },
    Story(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub window: Window,
    pub column: Option<ScreenColumn>,
    pub filter: FieldState,
}

impl Feature for TestingScreen {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = std::convert::Infallible;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, Request>,
    ) -> bool {
        match message {
            Message::Recordings(names) => state.recordings.clone_from(names),
            Message::Stories(names) => state.stories = Arc::clone(names),
            Message::Story(name) => {
                if state.stories.contains(name) {
                    context.request(Request::Story(name.clone()));
                }
                return false;
            }
            Message::Seat(seat) => state.seat = *seat,
            Message::Filter(text) => state.filter.clone_from(text),
            Message::Replay { recording } => {
                if let Some(name) = state.recordings.get(usize::from(*recording)) {
                    context.request(Request::Replay {
                        name: name.clone(),
                        seat: state.seat,
                    });
                }
                return false;
            }
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs) -> Inputs {
        inputs.clone()
    }

    fn view(state: &State, inputs: Inputs) -> Element {
        view::testing(state, &inputs)
    }
}
