//! The wire types Beryl's interfaces read: the [`PlayerView`](protocol::PlayerView) protocol,
//! the scripts that animate one view into the next ([`script`]), prompts ([`prompt`]) and the
//! app's IPC names and messages ([`ipc`]). `beryl_session` builds these from the engine and
//! re-exports every one of them at the same path, so the web interface's generated types and
//! the app do not change. This crate depends on no rules or card crate, so a client that only
//! reads views (Beryl 3D) does not link the engine.

pub mod ipc;
pub mod prompt;
pub mod protocol;
pub mod script;

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use protocol::{CandidateIndex, DecisionId, PlayerView, SessionId};

/// An agent a human can play against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub name: String,
    /// What a human calls it.
    pub title: String,
    pub description: String,
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
    /// The error for a failed read or write of `path`.
    pub fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SessionError + '_ {
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

/// The UI's request for a game: the default deck's mirror against `agent`, the human in seat 0
/// under the smart stops.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub agent: String,
    /// The game's seed; none picks one from the clock.
    pub seed: Option<u32>,
    /// Deal the game the AI judges fairest of many, searched from the seed ([`crate::fair`]).
    #[serde(default)]
    pub fair: bool,
}

