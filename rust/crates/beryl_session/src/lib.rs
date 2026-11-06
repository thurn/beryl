//! Game sessions (architecture §19.2, decisions D12 to D14): a human against an agent, one
//! game at a time on its own thread ([`Sessions`]), the [`PlayerView`] protocol the UI draws
//! from, the app's IPC names ([`ipc`]) and the replay files that reproduce a game.
//!
//! The UI never sees a `State`: each view is built from the human's `Observation`, so it
//! holds only what they know.

mod game;
pub mod ipc;
pub mod preview;
pub mod protocol;
pub mod replay;
mod session;
mod sessions;
pub mod typescript;

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use beryl_engine::Seat;
pub use game::{Answer, Game, OpenDecision, with_stops};
pub use protocol::{CandidateIndex, DecisionId, PlayerView, SessionId, Status, StopsSetting};
pub use replay::ReplayFile;
pub use sessions::{Sessions, StartRequest};

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

/// An agent a human can play against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub name: String,
    pub description: String,
}

/// Every agent, in the order a human is offered them.
pub fn agents() -> Vec<AgentView> {
    beryl_search::AGENTS
        .iter()
        .map(|a| AgentView {
            name: a.name.to_owned(),
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
            SessionError::Ended => write!(f, "the session has ended"),
            SessionError::BadReplay { reason } => write!(f, "not a replay: {reason}"),
            SessionError::Io { reason } => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for SessionError {}
