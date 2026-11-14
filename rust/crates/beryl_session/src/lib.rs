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
#[cfg(test)]
mod fuzz;
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
pub mod typescript;
mod wire_fingerprint;

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use beryl_engine::Seat;
use beryl_engine::{GameTable, State};
use beryl_search::GameAgent;
pub use game::{Answer, Game, OpenDecision, UNDO_POINTS, with_stops};
pub use prompt::trigger_to_stack;
pub use protocol::{CandidateIndex, DecisionId, PlayerView, SessionId, Status, StopsSetting};
pub use replay::ReplayFile;
pub use session::THREAD as SESSION_THREAD;
pub use sessions::{Sessions, StartRequest};

/// A 64-bit FNV-1a hash of the generated TypeScript protocol, in lowercase hexadecimal.
pub const WIRE_HASH: &str = env!("BERYL_WIRE_HASH");

/// The wire fingerprint extended with additional definitions in order.
pub fn wire_hash(extra: &[&str]) -> String {
    let mut hash = u64::from_str_radix(WIRE_HASH, 16).expect("build.rs emits a hex hash");
    for definition in extra {
        // Prefix each definition with its length so the hash input preserves the boundaries.
        hash = wire_fingerprint::extend(hash, &(definition.len() as u64).to_le_bytes());
        hash = wire_fingerprint::extend(hash, definition.as_bytes());
    }
    format!("{hash:016x}")
}

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

/// An agent a human can play against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub name: String,
    /// What a human calls it.
    pub title: String,
    pub description: String,
}

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

/// A view of one session, as the app sends it to the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameUpdate {
    /// Session numbers only grow, so the UI can drop a view from a replaced game.
    pub session: SessionId,
    pub view: PlayerView,
}

/// Why a session could not start or take a command. Commands return it to the UI as is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionError {
    UnknownAgent {
        name: String,
    },
    UnknownCard {
        name: String,
    },
    /// The decklists do not make a game.
    Deck {
        reason: String,
    },
    /// The command names a session other than the one in progress: it was replaced, or never
    /// started.
    StaleSession {
        session: SessionId,
    },
    /// No decision of the human's is open: the agent is deciding, the open one was answered
    /// already, or the game is over.
    NotDeciding,
    /// The answer names a decision other than the open one.
    StaleDecision {
        decision: DecisionId,
        open: DecisionId,
    },
    NoSuchCandidate {
        index: CandidateIndex,
        candidates: usize,
    },
    /// The open decision is not part of a cast the human may take back.
    NotCancellable,
    /// The human has no answer to take back: none since the opponent last decided or since
    /// something came out they could not know before.
    NothingToUndo,
    /// The open decision is no priority from which to pass on to a later stop.
    NoPassOn,
    /// The session does not hold the state of the open decision's prompt: a click has nothing
    /// to change, and the decision is answered by its candidates.
    PromptNotHeld,
    /// The click names a state of the open prompt other than the one it is in.
    StalePrompt {
        revision: u32,
        open: u32,
    },
    /// The session's thread has ended.
    Ended,
    BadReplay {
        reason: String,
    },
    Io {
        reason: String,
    },
}

impl SessionError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SessionError + '_ {
        move |e| SessionError::Io {
            reason: format!("{}: {e}", path.display()),
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::UnknownAgent { name } => write!(f, "no agent is named {name:?}"),
            SessionError::UnknownCard { name } => write!(f, "no card is named {name:?}"),
            SessionError::Deck { reason } => write!(f, "the decks do not make a game: {reason}"),
            SessionError::StaleSession { session } => {
                write!(f, "game {} is not in progress", session.0)
            }
            SessionError::NotDeciding => write!(f, "no decision is open"),
            SessionError::StaleDecision { decision, open } => write!(
                f,
                "decision {} of game {} is not the open one, {} of game {}",
                decision.seq, decision.session.0, open.seq, open.session.0
            ),
            SessionError::NoSuchCandidate { index, candidates } => {
                write!(f, "candidate {} of {candidates}", index.0)
            }
            SessionError::NotCancellable => write!(f, "nothing being cast can be taken back"),
            SessionError::NothingToUndo => write!(f, "no answer to take back"),
            SessionError::NoPassOn => write!(f, "no later stop to pass on to"),
            SessionError::PromptNotHeld => write!(f, "the open prompt takes no clicks"),
            SessionError::StalePrompt { revision, open } => write!(
                f,
                "state {revision} of the open prompt is not the one it is in, {open}"
            ),
            SessionError::Ended => write!(f, "the session has ended"),
            SessionError::BadReplay { reason } => write!(f, "not a replay: {reason}"),
            SessionError::Io { reason } => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for SessionError {}
