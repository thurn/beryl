//! The playground dock: while a card's playground example is played, a small control docked
//! at the top edge of the table, at its left end, names it, `<card> · <example> (n/N)`, and opens as a dropdown
//! of the card's examples. Choosing one ends the game and starts that example; "Back" returns
//! to the testing screen. On the desktop `]` and `[` move to the next and the previous example,
//! wrapping around, and `R` starts the current one again (`SHORTCUTS`).
//!
//! Arena has no such control: it is a debug control of the tools build, listed in UI
//! Conventions under "Not seen in Arena", built from the lobby's deck picker's buttons and the
//! testing screen's list rows.

use beryl_protocol::SessionId;
use game_table::table::Window;
use glam::Vec2;
use kit_base::tokens::{color, size, space, type_style};
use kit_scene::{Context, Feature, Shortcut};
use kit_ui::element::{Anchor, Element, column, text};
use kit_ui::widgets::{button, list};

use super::playground::Card;
use super::view::list_row;

/// The name of the dock's element, and of its open list.
pub const DOCK: &str = "playground dock";
pub const ENTRIES: &str = "playground examples";
/// The words of the list's last entry, which returns to the testing screen.
pub const BACK: &str = "Back";

/// The dock's keys, in `Message` order: next, previous, restart.
pub const SHORTCUTS: [Shortcut; 3] = [
    Shortcut::binding("]", "Next example"),
    Shortcut::binding("[", "Previous example"),
    Shortcut::binding("R", "Restart example"),
];

/// How far below the dock's top the open list or an error stands: the button's height and a
/// small gap.
const LIST_DROP: f32 = size::BUTTON_HEIGHT + space::S;

pub struct PlaygroundDock;

#[derive(Debug, Default)]
pub struct State {
    /// The card whose examples are played, and the index of the one played now.
    pub card: Option<(Card, usize)>,
    /// The session the example now played was started in, once the session has said so. The
    /// dock is shown only over its views.
    pub session: Option<SessionId>,
    /// Whether the list of examples is open.
    pub open: bool,
    /// Why the session did not start the example last chosen.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// Play `card`'s example at the index, as the testing screen chose it.
    Play(Card, u16),
    /// The session started the example in this session, or says why it did not.
    Started(Result<SessionId, String>),
    Toggle,
    Choose(u16),
    Next,
    Previous,
    Restart,
    Back,
}

/// What the dock asks of the session: start the card's variant, ending the game in progress.
pub enum Command {
    Start { card: String, variant: String },
}

pub enum Request {
    /// Show the game screen, where the example's views will be drawn.
    Game,
    /// Return to the testing screen.
    Testing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub window: Window,
    /// Where the dock's top left corner stands, in logical points: on the table's top edge,
    /// right of the game log's button, so that its open list falls clear of the opponent's
    /// life total at the edge's middle.
    pub at: Vec2,
}

impl State {
    /// Whether the dock is shown over the views of `session`.
    #[must_use]
    pub fn over(&self, session: Option<SessionId>) -> bool {
        self.card.is_some() && self.session.is_some() && self.session == session
    }
}

impl Feature for PlaygroundDock {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = Command;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Command, Request>,
    ) -> bool {
        let count = state
            .card
            .as_ref()
            .map_or(0, |(card, _)| card.examples.len());
        let current = state.card.as_ref().map_or(0, |(_, index)| *index);
        let to = match message {
            Message::Play(card, index) => {
                state.card = Some((card.clone(), usize::from(*index)));
                Some(usize::from(*index))
            }
            Message::Started(started) => {
                state.session = started.as_ref().ok().copied();
                state.error = started.as_ref().err().cloned();
                return true;
            }
            Message::Toggle => {
                state.open = !state.open;
                return true;
            }
            Message::Back => {
                *state = State::default();
                context.request(Request::Testing);
                return true;
            }
            Message::Choose(index) => Some(usize::from(*index)),
            Message::Next if count > 0 => Some((current + 1) % count),
            Message::Previous if count > 0 => Some((current + count - 1) % count),
            Message::Restart => Some(current),
            Message::Next | Message::Previous => None,
        };
        let Some((card, index)) = state.card.as_mut() else {
            return false;
        };
        let Some(example) = to.and_then(|to| card.examples.get(to)) else {
            return false;
        };
        *index = to.unwrap_or_default();
        state.open = false;
        state.session = None;
        state.error = None;
        context.command(Command::Start {
            card: card.slug.clone(),
            variant: example.name.clone(),
        });
        context.request(Request::Game);
        true
    }

    fn view_key(_: &State, inputs: &Inputs) -> Inputs {
        inputs.clone()
    }

    fn view(state: &State, inputs: Inputs) -> Element {
        let Some((card, index)) = &state.card else {
            return column().name(DOCK);
        };
        let mut header = button(&card.caption(*index))
            .selected(state.open)
            .on_press(Message::Toggle);
        let rows = (0u16..)
            .zip(&card.examples)
            .map(|(at, example)| {
                list_row(Message::Choose(at), &example.text(), None)
                    .selected(usize::from(at) == *index)
            })
            .chain(std::iter::once(list_row(Message::Back, BACK, None)));
        // The button and the open list each stand at their own point of the window, and the
        // holder has no size, so that only the two take presses: a column as wide as the
        // button would also cover the table's controls beside the narrower list.
        header.anchor = Some(Anchor::Point(inputs.at, Vec2::ZERO));
        let entries = state.open.then(|| {
            let mut entries = list(ENTRIES, rows)
                .padding(space::S)
                .background(color::SURFACE)
                .max_height(inputs.window.size.height / 2.0);
            entries.anchor = Some(Anchor::Point(
                inputs.at + Vec2::new(0.0, LIST_DROP),
                Vec2::ZERO,
            ));
            entries
        });
        let error = (state.error.as_deref()).map(|error| {
            let mut note = text(error).style(type_style::BODY).color(color::EMBER);
            note.anchor = Some(Anchor::Point(
                inputs.at + Vec2::new(0.0, LIST_DROP),
                Vec2::ZERO,
            ));
            note
        });
        column()
            .name(DOCK)
            .width(0.0)
            .height(0.0)
            .child(header)
            .children(error)
            .children(entries)
    }
}
