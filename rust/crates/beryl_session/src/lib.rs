//! Game sessions (architecture §19.2, decisions D12 to D14): a human against an agent, one
//! game at a time on its own thread ([`Sessions`]), the [`PlayerView`] protocol the UI draws
//! from, the app's IPC names ([`ipc`]) and the replay files that reproduce a game.
//!
//! The UI never sees a `State`: each view is built from the human's `Observation`, so it
//! holds only what they know.

pub mod ai;
pub mod bugs;
pub mod card_records;
pub mod checkpoint;
pub mod corpus;
pub mod fair;
mod game;
pub mod ipc;
pub mod journal;
mod ledger;
pub mod migrate;
pub mod perturb;
pub mod preview;
pub mod prompt;
pub mod protocol;
pub mod replay;
pub mod save;
pub mod script;
mod session;
mod sessions;

pub use beryl_engine::Seat;
use beryl_engine::{GameTable, State};
pub use beryl_protocol::{
    AgentView, GameUpdate, SessionError, SessionIds, StartRequest, WIRE_HASH, wire_hash,
};
use beryl_search::GameAgent;
pub use game::{Answer, Game, OpenDecision, UNDO_POINTS, with_stops};
pub use prompt::trigger_to_stack;
pub use protocol::{CandidateIndex, DecisionId, PlayerView, SessionId, Status, StopsSetting};
pub use replay::ReplayFile;
pub use session::THREAD as SESSION_THREAD;
pub use sessions::Sessions;

/// How to start a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartGame {
    /// The game's seed; the agent's is derived from it.
    pub seed: u64,
    /// Each seat's decklist: (count, card name).
    pub decks: [Vec<(u8, String)>; 2],
    /// The human's seat.
    pub human: Seat,
    /// The name of the agent in the other seat ([`beryl_search::AGENTS`]).
    pub opponent: String,
    pub stops: StopsSetting,
}

/// A game set up by hand rather than dealt from a seed (UI fixtures): its table, the state
/// before its next decision, and the agents that play it.
pub struct Prepared {
    pub table: GameTable,
    pub state: State,
    pub human: Seat,
    pub stops: StopsSetting,
    /// The agent in the other seat.
    pub opponent: Box<dyn GameAgent + Send>,
    /// The agent answering the human's decisions in their place.
    pub stand_in: Box<dyn GameAgent + Send>,
    /// Once this says so, the stand-in stops and the human answers their own decisions from
    /// there on (a spec's moment, played live from where its script ends). None keeps the
    /// stand-in to the end.
    pub hand_over: Option<HandOver>,
    /// Both seats follow scripts (a spec's moment, a fixture's board), so neither has the
    /// activations that would do nothing hidden
    /// ([`Profile::hides_pointless`](beryl_engine::Profile)): either seat's script may take
    /// one the human could, and a moment plays the same seen from either chair.
    pub scripted: bool,
}

/// Whether a stand-in should hand the human's decisions back to them ([`Prepared::hand_over`]).
pub type HandOver = Box<dyn Fn() -> bool + Send>;

/// Both seats' views of a prepared review game, in the prepared human's seat order.
pub type ReviewViews = Box<dyn Fn(SessionId, [PlayerView; 2]) + Send>;

/// The view of each update as the seat across the table from the human sees it, sent just
/// before the human's view of the same update ([`Sessions::with_across`]).
pub type AcrossViews = Box<dyn Fn(PlayerView) + Send>;

/// Every agent, in the order a human is offered them.
pub fn agents() -> Vec<AgentView> {
    beryl_search::AGENTS
        .iter()
        .map(|a| AgentView {
            name: a.name.to_owned(),
            title: a.title.to_owned(),
            description: a.description.to_owned(),
        })
        .collect()
}
