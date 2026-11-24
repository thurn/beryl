//! Token search and the custom token form.

mod view;
pub use view::{Inputs, ViewKey};

use crate::manual::tokens::{CustomToken, TokenSearch as Search};
use beryl_protocol::{PlayerView, ipc::decks::CardSearchHit};
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::Element;

#[derive(Debug, Default)]
pub struct State {
    pub search: Option<Search>,
    pub custom: Option<CustomToken>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Open(Search),
    Custom(CustomToken),
    Reset,
    Query(String),
    Found(String, Vec<CardSearchHit>),
    Choose(u16),
    Color(u8),
    Create,
    Close,
}

pub struct TokenSearch;

/// Moves the highlight among the rows derived from the latest table view.
pub struct Highlight<'a> {
    pub down: bool,
    pub view: &'a PlayerView,
}

impl StateUpdate<TokenSearch> for Highlight<'_> {
    type Output = ();

    fn apply(self, state: &mut State) -> (bool, ()) {
        let Some(search) = &mut state.search else {
            return (false, ());
        };
        let previous = search.highlight;
        search.move_highlight(self.down, self.view);
        (search.highlight != previous, ())
    }
}

impl Feature for TokenSearch {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = String;
    type Request = Message;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, Self::Request>,
    ) -> bool {
        match message {
            Message::Open(search) => state.search = Some(search.clone()),
            Message::Custom(custom) => state.custom = Some(custom.clone()),
            Message::Reset => *state = State::default(),
            Message::Query(query) => {
                let Some(open) = state.search.as_mut() else {
                    return false;
                };
                open.set_query(query);
                if let Some(text) = open.wants(query) {
                    open.asked = Some(text.clone());
                    context.command(text);
                }
            }
            Message::Found(query, hits) => {
                let Some(open) = state.search.as_mut() else {
                    return false;
                };
                open.answer = Some((query.clone(), hits.clone()));
            }
            Message::Color(at) => {
                if let Some(color) = state
                    .custom
                    .as_mut()
                    .and_then(|form| form.colors.get_mut(usize::from(*at)))
                {
                    *color = !*color;
                }
            }
            Message::Close => {
                state.search = None;
                state.custom = None;
                context.request(Message::Close);
            }
            Message::Choose(_) | Message::Create => {
                context.request(message.clone());
                return false;
            }
        }
        true
    }

    fn view_key(state: &State, inputs: &Inputs<'_>) -> ViewKey {
        view::key(state, inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(state, inputs)
    }
}
