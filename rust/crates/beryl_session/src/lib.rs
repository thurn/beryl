//! Game sessions (architecture §19.2, decisions D12 to D14): a human against an agent, the
//! game on its own thread ([`GameSession`]), the [`PlayerView`] protocol the UI draws from,
//! and the replay files that reproduce a game.
//!
//! The UI never sees a `State`: each view is built from the human's `Observation`, so it
//! holds only what they know.

mod game;
pub mod protocol;
pub mod replay;
mod session;
pub mod typescript;

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use game::Game;
pub use protocol::{PlayerView, StopsSetting};
pub use replay::ReplayFile;
pub use session::GameSession;

/// How to start a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartGame {
    /// The game's seed; the agent's is derived from it.
    pub seed: u64,
    /// Each seat's decklist: (count, card name).
    pub decks: [Vec<(u8, String)>; 2],
    /// The human's seat.
    pub human: u8,
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
    pub session: u32,
    pub view: PlayerView,
}

/// Why a session could not start or take a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    UnknownAgent(String),
    UnknownCard(String),
    /// The decklists do not make a game.
    Deck(String),
    /// No decision of the human's is open: the agent is deciding, or the game is over.
    NotDeciding,
    /// The answer names a decision other than the open one.
    StaleDecision {
        seq: u32,
        open: u32,
    },
    NoSuchCandidate {
        index: usize,
        candidates: usize,
    },
    /// The session's thread has ended.
    Ended,
    BadReplay(String),
    Io(String),
}

impl SessionError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SessionError + '_ {
        move |e| SessionError::Io(format!("{}: {e}", path.display()))
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::UnknownAgent(name) => write!(f, "no agent is named {name:?}"),
            SessionError::UnknownCard(name) => write!(f, "no card is named {name:?}"),
            SessionError::Deck(why) => write!(f, "the decks do not make a game: {why}"),
            SessionError::NotDeciding => write!(f, "no decision is open"),
            SessionError::StaleDecision { seq, open } => {
                write!(f, "decision {seq} is not the open one, {open}")
            }
            SessionError::NoSuchCandidate { index, candidates } => {
                write!(f, "candidate {index} of {candidates}")
            }
            SessionError::Ended => write!(f, "the session has ended"),
            SessionError::BadReplay(why) => write!(f, "not a replay: {why}"),
            SessionError::Io(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for SessionError {}
