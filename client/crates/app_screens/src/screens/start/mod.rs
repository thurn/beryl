//! The start screen's new-game controls, the AI's effort and navigation. The opponent is
//! always the agent the host offers first: the ladder's champion.

mod keys;
mod view;

pub use keys::{KeyInputs, SHORTCUTS, press as shortcut_press};

use beryl_protocol::protocol::Status;
use beryl_protocol::{AgentView, DECKS, SessionId, StartRequest};
use game_manual::screens::table_entries::{INVITE, NAME, TableEntries};
use game_table::table::Window;
use kit_scene::{Context, Feature};
use kit_ui::element::{Action, Element};
use kit_ui::widgets::screen::ScreenColumn;
use kit_ui::widgets::{FieldState, Fields};

use super::Screen;

pub const SEED: &str = "seed";

/// The engine steps a decision the opponent searches for each level of the effort slider:
/// 400k, so that the slider's 100 is 40M and its default is 30M.
const WORK_PER_LEVEL: u64 = 400_000;

/// The effort slider's level when the screen opens: 75, which is 30M engine steps a decision,
/// the work the champion searched with before the slider existed.
pub const DEFAULT_EFFORT: u8 = 75;

/// The effort slider's lowest level, 5, which is 2M engine steps a decision: no search at all
/// would not be the champion.
const LEAST_EFFORT: u8 = kit_ui::widgets::STEP;

pub struct StartScreen;

#[derive(Debug)]
pub struct State {
    /// The agent the host offers first, which every game the screen starts is played against.
    agent: Option<String>,
    /// The effort slider's level, from `LEAST_EFFORT` to 100.
    effort: u8,
    /// The index into `DECKS` of the chosen deck.
    deck: usize,
    fair: bool,
    confirming: bool,
    seed: String,
    /// What a view from a new session should do while the start screen is up.
    waiting: Waiting,
}

/// What the start screen waits for a new session's view to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Waiting {
    /// Nothing: a view arriving now leaves the screen as it is.
    #[default]
    Nothing,
    /// Take the player from Start to the game the host resumed at launch.
    Resume,
    /// Enter the game of a fair deal, which the session judges for seconds before the game
    /// starts. Meanwhile the start screen stays up with its start button disabled, rather than
    /// the game screen showing the previous game.
    Deal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Agents(Vec<AgentView>, bool),
    /// A view arrived from a session newer than any the client had shown: the first view at
    /// launch, or the first of a game just started.
    NewSession,
    Leave,
    Open(Screen),
    Start(Option<SessionId>),
    /// Set the effort slider to this level. The slider's own press carries its current level.
    Effort(u8),
    /// Choose the deck at this index of `DECKS`.
    Deck(usize),
    FairDeal,
    Seed(String),
}

pub enum Command {
    Concede(SessionId),
    Start(StartRequest),
}

pub enum Request {
    Open(Screen),
    /// Show the start screen until a session view arrives.
    AwaitGame,
    /// Enter the new game after sending the session commands.
    EnterGame,
    /// Let the first session view show the resumed game.
    ResumeGame,
}

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub tooltip: kit_base::data::flat::Tooltip,
    pub window: Window,
    pub column: Option<ScreenColumn>,
    pub in_progress: bool,
    pub session: Option<SessionId>,
    pub has_recordings: bool,
    pub entries: &'a TableEntries,
    pub fields: &'a Fields,
}

#[derive(PartialEq)]
pub struct ViewKey {
    tooltip: kit_base::data::flat::Tooltip,
    window: Window,
    column: Option<ScreenColumn>,
    in_progress: bool,
    session: Option<SessionId>,
    has_recordings: bool,
    entries: TableEntries,
    seed: FieldState,
    name: FieldState,
    invite: FieldState,
}

impl Feature for StartScreen {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Command;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Command, Request>,
    ) -> bool {
        match message {
            Message::Agents(agents, before_view) => {
                state.agent = agents.first().map(|agent| agent.name.clone());
                if *before_view {
                    state.waiting = Waiting::Resume;
                    context.request(Request::AwaitGame);
                }
            }
            Message::NewSession => {
                match std::mem::take(&mut state.waiting) {
                    Waiting::Deal => {
                        context.request(Request::EnterGame);
                        return true;
                    }
                    Waiting::Resume => context.request(Request::ResumeGame),
                    Waiting::Nothing => {}
                }
                return false;
            }
            Message::Leave => {
                state.waiting = Waiting::Nothing;
                return false;
            }
            Message::Open(screen) => {
                state.waiting = Waiting::Nothing;
                context.request(Request::Open(*screen));
                return false;
            }
            Message::Start(_) if state.dealing() => return false,
            Message::Start(going_on) => {
                let Some(agent) = state.agent.clone() else {
                    return false;
                };
                let Ok(seed) = seed(&state.seed) else {
                    return false;
                };
                if let Some(session) = going_on {
                    if !state.confirming {
                        state.confirming = true;
                        return true;
                    }
                    context.command(Command::Concede(*session));
                }
                let fair = state.fair;
                context.command(Command::Start(StartRequest {
                    agent,
                    seed,
                    fair,
                    deck: state.deck().key.to_owned(),
                    work: Some(work(state.effort)),
                }));
                state.confirming = false;
                if fair {
                    // The game screen would show the previous game until the deal is judged.
                    state.waiting = Waiting::Deal;
                } else {
                    state.waiting = Waiting::Nothing;
                    context.request(Request::EnterGame);
                }
            }
            Message::Effort(level) => state.effort = (*level).clamp(LEAST_EFFORT, 100),
            Message::Deck(at) => {
                if *at < DECKS.len() {
                    state.deck = *at;
                }
            }
            Message::FairDeal => state.fair = !state.fair,
            Message::Seed(text) => state.seed.clone_from(text),
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs) -> ViewKey {
        ViewKey {
            tooltip: inputs.tooltip,
            window: inputs.window,
            column: inputs.column,
            in_progress: inputs.in_progress,
            session: inputs.session,
            has_recordings: inputs.has_recordings,
            entries: inputs.entries.clone(),
            seed: inputs.fields.get(SEED),
            name: inputs.fields.get(NAME),
            invite: inputs.fields.get(INVITE),
        }
    }

    fn view(state: &State, inputs: Inputs) -> Element {
        view::start(state, &inputs)
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            agent: None,
            effort: DEFAULT_EFFORT,
            deck: 0,
            fair: false,
            confirming: false,
            seed: String::new(),
            waiting: Waiting::Nothing,
        }
    }
}

impl State {
    /// The chosen deck.
    fn deck(&self) -> beryl_protocol::DeckChoice {
        DECKS[self.deck.min(DECKS.len() - 1)]
    }

    /// Whether a fair deal was asked for and its game has sent no view yet.
    fn dealing(&self) -> bool {
        self.waiting == Waiting::Deal
    }
}

/// The engine steps a decision the opponent searches at effort `level`.
#[must_use]
pub fn work(level: u8) -> u64 {
    u64::from(level.clamp(LEAST_EFFORT, 100)) * WORK_PER_LEVEL
}

/// The effort slider's label for `level`: its work in millions of engine steps, as `30M`.
#[must_use]
pub fn effort_label(level: u8) -> String {
    format!("{}M", work(level) / 1_000_000)
}

/// The level the effort slider shows, if `action` is its press.
#[must_use]
pub fn effort_level(action: &Action) -> Option<u8> {
    match action.downcast_ref::<Message>()? {
        Message::Effort(level) => Some(*level),
        _ => None,
    }
}

/// What the seed field holds when it is neither blank nor a seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotASeed;

/// The seed a seed field holding `words` asks for: `Ok(None)` while it is blank, which deals
/// from the clock, and an error unless it is a whole number from 0 to 2^32 - 1.
///
/// # Errors
///
/// When `words` is neither blank nor such a number.
pub fn seed(words: &str) -> Result<Option<u32>, NotASeed> {
    let words = words.trim();
    if words.is_empty() {
        return Ok(None);
    }
    words.parse().map(Some).map_err(|_| NotASeed)
}

/// Whether `view` is of an engine game that goes on, which a new game concedes first: one
/// waiting on a player or playing through a step. A game that is over, a session that stopped
/// and the manual table (`Status::Free`) are not.
#[must_use]
pub fn in_progress(view: Option<&beryl_protocol::PlayerView>) -> bool {
    view.is_some_and(|view| {
        matches!(
            view.status,
            Status::Deciding { .. } | Status::Thinking | Status::Playing
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_effort_is_30m_and_the_slider_runs_from_2m_to_40m() {
        assert_eq!(work(DEFAULT_EFFORT), 30_000_000);
        assert_eq!(effort_label(DEFAULT_EFFORT), "30M");
        assert_eq!(effort_label(0), "2M");
        assert_eq!(effort_label(100), "40M");
    }
}
