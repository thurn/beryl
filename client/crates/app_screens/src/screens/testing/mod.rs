//! The recordings browser and the seat to replay from, and the card playground: a field that
//! finds a card, the list of its playground examples, and the dock that switches between them
//! while one is played (`dock`).

pub mod dock;
pub mod playground;
mod view;

use std::sync::Arc;

use beryl_protocol::protocol::Side;
use game_table::table::Window;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use kit_ui::widgets::FieldState;
use kit_ui::widgets::screen::ScreenColumn;

pub const FILTER: &str = "filter";
/// The field a card for the playground is typed into.
pub const CARD: &str = "card";

pub struct TestingScreen;

#[derive(Debug)]
pub struct State {
    pub recordings: Vec<String>,
    stories: Arc<[String]>,
    seat: Side,
    filter: String,
    /// What the card field holds.
    card: String,
    /// The card the session found for what the card field holds, if any.
    found: Option<String>,
    /// The examples of the card last picked, or why it has none.
    examples: Option<Result<playground::Card, String>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            recordings: Vec::new(),
            stories: Arc::default(),
            seat: Side::You,
            filter: String::new(),
            card: String::new(),
            found: None,
            examples: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Recordings(Vec<String>),
    Stories(Arc<[String]>),
    Story(String),
    Replay {
        recording: u16,
    },
    Seat(Side),
    Filter(String),
    /// The card field changed.
    Card(String),
    /// The session's closest card for what the card field held.
    Found {
        text: String,
        card: Option<String>,
    },
    /// Pick the card found, which lists its examples.
    Pick,
    /// The examples of the card picked, or why it has none.
    Examples(Result<playground::Card, String>),
    /// Play the picked card's example at this index.
    Play(u16),
}

pub enum Request {
    Replay {
        name: String,
        seat: Side,
    },
    Story(String),
    /// Find the card closest to this text.
    Find(String),
    /// List this card's examples.
    Examples(String),
    /// Play the card's example at the index.
    Play(playground::Card, u16),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub window: Window,
    pub column: Option<ScreenColumn>,
    pub filter: FieldState,
    pub card: FieldState,
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
            Message::Card(text) => {
                state.card.clone_from(text);
                context.request(Request::Find(text.clone()));
            }
            Message::Found { text, card } => {
                if *text != state.card {
                    return false;
                }
                state.found.clone_from(card);
            }
            Message::Pick => {
                let Some(card) = &state.found else {
                    return false;
                };
                context.request(Request::Examples(card.clone()));
                state.examples = None;
            }
            Message::Examples(examples) => state.examples = Some(examples.clone()),
            Message::Play(index) => {
                if let Some(Ok(card)) = &state.examples {
                    context.request(Request::Play(card.clone(), *index));
                }
                return false;
            }
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
