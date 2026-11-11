//! The entries of `data/blunders.json` and the rebuilt boards they may carry.

use std::fs;

use beryl_session::StopsSetting;
use beryl_session::replay::Entry as LogEntry;
use serde::{Deserialize, Serialize};

use super::root;

/// One blunder: a position, and what the agent may and must play there.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    /// The incident write-up, `docs/incidents/<incident>.md`.
    pub incident: String,
    /// The class of the blunder, not only the position (§5): what the fix must address.
    pub class: String,
    pub source: Source,
    /// The seat of the agent that blundered.
    pub seat: u8,
    /// The decision: its turn and its index among that turn's decisions, from 0, counting both
    /// players' and counting from the board's start for a rebuilt board.
    pub turn: u16,
    pub index: u32,
    /// The label of the record's answer there, which the replay must find: the game's own
    /// play, so a replay that plays another game is caught.
    pub played: String,
    /// The kinds of decision (`Attacker`, `AttackTarget`, ...) the agent goes on through after
    /// the first, while it is the one deciding; its choices over all of them are judged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<String>,
    /// Labels the agent may not choose.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden: Vec<String>,
    /// Labels the agent must choose, each as often as it is listed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    /// Why, in a sentence.
    pub why: String,
}

/// Where an entry's position comes from, a path from the repository's root.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Source {
    /// An incident's replay file.
    Replay(String),
    /// A rebuilt board ([`Rebuilt`]).
    Board(String),
    /// A seed and semantic engine choices from a reviewed hunt, using SMART for both seats.
    Seeded(String),
}

/// A hunt's minimal reproducible fixture, including the recorded blunder itself.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Seeded {
    pub(super) seed: u64,
    pub(super) history: Vec<beryl_session::save::Chosen>,
}

/// A rebuilt position: the board and the record's answers from it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rebuilt {
    pub board: Board,
    pub path: Vec<LogEntry>,
}

/// A game as it stood at its active player's priority with the stack empty.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Board {
    pub seed: u64,
    pub turn: u16,
    pub active: u8,
    /// The step, as `TurnStep` names it.
    pub step: String,
    pub human: u8,
    /// The human's stops then.
    pub stops: StopsSetting,
    pub players: [Side; 2],
    /// Every permanent, in the order they entered.
    pub battlefield: Vec<Permanent>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Side {
    pub life: i16,
    pub hand: Vec<InHand>,
    /// Top first.
    pub graveyard: Vec<String>,
    pub exile: Vec<Exiled>,
    /// Top first: the whole library.
    pub library: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InHand {
    pub card: String,
    /// The other player knows it.
    #[serde(default)]
    pub shown: bool,
}

/// A card in exile and the permanent, by its place in [`Board::battlefield`], it was exiled
/// with ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)) or until
/// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exiled {
    pub card: String,
    #[serde(default)]
    pub with: Option<u16>,
    #[serde(default)]
    pub until: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a permanent's statuses, each yes or no in the rules"
)]
pub struct Permanent {
    pub card: String,
    pub owner: u8,
    pub controller: u8,
    #[serde(default)]
    pub token: bool,
    #[serde(default)]
    pub one_one: bool,
    /// 0 front, 1 back, 255 face down.
    #[serde(default)]
    pub face: u8,
    #[serde(default)]
    pub tapped: bool,
    #[serde(default)]
    pub sick: bool,
    #[serde(default)]
    pub damage: u8,
    /// Each kind, as `CounterKind` names it, and its count.
    #[serde(default)]
    pub counters: Vec<(String, u8)>,
    #[serde(default)]
    pub attached_to: Option<u16>,
}

/// The entries of `data/blunders.json`.
pub fn entries() -> Result<Vec<Entry>, String> {
    let path = root().join("data/blunders.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
