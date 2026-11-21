//! The manual table's controls and transient interaction state.

mod view;

pub use view::{Inputs, ViewKey};

use crate::manual::{ActionSource, Pending, arrows, drop, opening, tokens::TokenChoice, viewers};
use beryl_protocol::protocol::{Answer, ObjectId, Side};
use glam::Vec2;
use kit_frame::frame::Sound;
use kit_scene::{Context, Feature};
use kit_ui::element::{Action, Element};
use kit_ui::layout::Shake;

/// A control of the table or a keyboard shortcut for a table command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    /// The command index and whether Shift makes the token face down.
    Shortcut(u16, bool),
    /// The icon at `at` of a step track, in the active player's rows if `active`
    /// (`turn::step_command`).
    Step { at: u8, active: bool },
    /// The Next button, which advances to the next phase.
    Next,
    /// The counter at `at` of `side`'s row of mana and player counters, which adds one
    /// (`pools`).
    Counter { side: Side, at: u8 },
    /// The minus button beside that counter in the phone's sheet, which removes one.
    CounterRemove { side: Side, at: u8 },
    /// Open the phone's sheet of `side`'s mana and counters, or close it (`pools::sheet`).
    ManaSheet(Option<Side>),
    /// Quit's question was answered Quit.
    Quit,
    /// Close the command list, ask prompt, token search, custom token form or order tray.
    Close,
    /// Apply the state nudge whose label has this id (`assists::labels`).
    Label(u32),
    /// A card of the sorting tray (`assists::sort_answer`).
    SortCard(ObjectId),
    /// The sorting tray's Done.
    SortDone,
    /// The sorting tray's Cancel.
    SortCancel,
    /// A button or card of the guided start.
    Opening(opening::OpeningPress),
    /// Leave the table, from the banner of a table the other player left.
    Leave,
    /// The pile viewer's "Group by type", which turns grouping on or off.
    ViewerGroup,
    /// The pile viewer's sort, which lists its cards in this order.
    ViewerSort(viewers::Sort),
    /// Close the search of a whole library, shuffling it first if `true` (Shuffle) or not
    /// (Don't shuffle).
    ViewerFinish(bool),
}

/// State that accompanies table input and its asynchronous replies.
#[derive(Debug, Default)]
pub struct State {
    pub last_token: Option<TokenChoice>,
    pub pending: std::collections::VecDeque<Pending>,
    pub modifiers: PointerModifiers,
    pub selection: Vec<ObjectId>,
    pub sweep: Option<(Vec2, Vec2)>,
    pub picking: Option<arrows::Pick>,
    pub landing: Option<drop::Destination>,
    /// The highest selected permanent on the window, which the selection's summary stands
    /// over (`selection::highest`).
    pub summary_over: Option<ObjectId>,
    /// What the open pile viewer shows beyond its pile: a library look, its sort and grouping,
    /// and whether it asks to shuffle (`viewers::PileViewer`).
    pub viewer: viewers::PileViewer,
    /// Whose mana and counters the phone's sheet shows, while it is open (`pools::sheet`). A
    /// new view leaves it open, so that a player adding mana sees the count change.
    pub mana_sheet: Option<Side>,
    pub quitting: bool,
    pub refused: bool,
    /// The card or button sending actions, remembered until their replies arrive.
    pub clicked: Option<ActionSource>,
    pub shaking: Option<(Action, Shake)>,
}

/// Modifiers recorded when the pointer was pressed.
#[derive(Debug, Default)]
pub struct PointerModifiers {
    pub shift: bool,
    /// Cmd on the Mac and Control elsewhere.
    pub meta: bool,
}

impl State {
    /// The sound played once for a refusal since the last frame.
    pub fn take_refusal(&mut self) -> Option<Sound> {
        std::mem::take(&mut self.refused).then(|| Sound {
            id: game_play::sound::id("no"),
            theirs: false,
        })
    }
}

/// Actions which need the client’s input route, layout or host.
pub enum Request {
    Shortcut(u16, bool),
    Step { at: u8, active: bool },
    Next,
    Counter { side: Side, at: u8, add: bool },
    Quit,
    CloseOverlay,
    SortCard(ObjectId),
    Leave,
    Viewer(viewers::ViewerControl),
}

pub struct TableScreen;

impl Feature for TableScreen {
    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Answer;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Answer, Request>,
    ) -> bool {
        let request = match *message {
            Message::ManaSheet(side) => {
                state.mana_sheet = side;
                return true;
            }
            Message::Quit => {
                state.quitting = false;
                context.request(Request::Quit);
                return true;
            }
            Message::Label(id) => {
                context.command(Answer::ApplyLabel { id });
                return false;
            }
            Message::SortDone | Message::SortCancel => {
                context.command(if *message == Message::SortDone {
                    Answer::Done
                } else {
                    Answer::Cancel
                });
                return false;
            }
            Message::Opening(press) => {
                for answer in press.answers() {
                    context.command(answer);
                }
                return false;
            }
            Message::Shortcut(index, shift) => Request::Shortcut(index, shift),
            Message::Step { at, active } => Request::Step { at, active },
            Message::Next => Request::Next,
            Message::Counter { side, at } => Request::Counter {
                side,
                at,
                add: true,
            },
            Message::CounterRemove { side, at } => Request::Counter {
                side,
                at,
                add: false,
            },
            Message::Close => Request::CloseOverlay,
            Message::SortCard(card) => Request::SortCard(card),
            Message::Leave => Request::Leave,
            Message::ViewerGroup => Request::Viewer(viewers::ViewerControl::Group),
            Message::ViewerSort(sort) => Request::Viewer(viewers::ViewerControl::Sort(sort)),
            Message::ViewerFinish(shuffle) => {
                Request::Viewer(viewers::ViewerControl::Finish { shuffle })
            }
        };
        context.request(request);
        false
    }

    fn view_key(state: &State, inputs: &Inputs<'_>) -> ViewKey {
        ViewKey::new(state, inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::table(state, inputs)
    }
}
