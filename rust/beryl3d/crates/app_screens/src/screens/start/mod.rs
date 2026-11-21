//! The start screen's opponent selection, new-game controls and navigation.

mod view;

use beryl_protocol::protocol::Status;
use beryl_protocol::{AgentView, SessionId, StartRequest};
use game_manual::screens::table_entries::{INVITE, NAME, TableEntries};
use game_table::table::Window;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use kit_ui::widgets::screen::ScreenColumn;
use kit_ui::widgets::{FieldState, Fields};

use super::Screen;

pub const SEED: &str = "seed";

/// The default opponent, when the host offers it.
const DEFAULT_AGENT: &str = "mcts-hidden";

pub struct StartScreen;

#[derive(Debug, Default)]
pub struct State {
    agents: Vec<AgentView>,
    agent: Option<u16>,
    fair: bool,
    confirming: bool,
    seed: String,
    /// Whether the host's first session view should take us from Start to Game.
    waiting_for_view: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Agents(Vec<AgentView>, bool),
    FirstView,
    Leave,
    Open(Screen),
    Start(Option<SessionId>),
    Agent(u16),
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
                state.agent = first_agent(agents);
                state.agents.clone_from(agents);
                if *before_view {
                    state.waiting_for_view = true;
                    context.request(Request::AwaitGame);
                }
            }
            Message::FirstView => {
                if std::mem::take(&mut state.waiting_for_view) {
                    context.request(Request::ResumeGame);
                }
                return false;
            }
            Message::Leave => {
                state.waiting_for_view = false;
                return false;
            }
            Message::Open(screen) => {
                state.waiting_for_view = false;
                context.request(Request::Open(*screen));
                return false;
            }
            Message::Start(going_on) => {
                let Some(agent) = state.agent.and_then(|at| state.agents.get(usize::from(at)))
                else {
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
                context.command(Command::Start(StartRequest {
                    agent: agent.name.clone(),
                    seed,
                    fair: state.fair,
                }));
                state.confirming = false;
                state.waiting_for_view = false;
                context.request(Request::EnterGame);
            }
            Message::Agent(agent) => state.agent = Some(*agent),
            Message::FairDeal => state.fair = !state.fair,
            Message::Seed(text) => state.seed.clone_from(text),
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs) -> ViewKey {
        ViewKey {
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

fn first_agent(agents: &[AgentView]) -> Option<u16> {
    if agents.is_empty() {
        return None;
    }
    let at = agents
        .iter()
        .position(|agent| agent.name == DEFAULT_AGENT)
        .unwrap_or(0);
    u16::try_from(at).ok()
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
