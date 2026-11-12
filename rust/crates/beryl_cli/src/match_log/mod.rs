//! Games between entrants, played and logged so a run resumes and replays: the ladder's
//! matches and the commands that read them (`gate`, `hunt`, `positions`, `screen`). The
//! layout is the retired long-think arena's (docs/longthink-run-003.md; docs/ai-strategy.md
//! §8), whose command is gone. A run directory holds:
//!
//! - `games.jsonl`, a line per finished game ([`GameRecord`]), the record that commits it;
//! - `decisions.jsonl`, a line per searched decision ([`DecisionRecord`]): the root's
//!   visits and mean rewards, training data, since a game replays from its seed;
//! - `logs/<game>.jsonl`, every decision's answer as it was made ([`game::Line`]), which a
//!   resumed game replays through, and `transcripts/<game>.txt`.

pub mod files;
pub mod game;

use std::path::PathBuf;

use beryl_search::AgentDecision;
use serde::{Deserialize, Serialize};

use game::GameDecision;

/// How a finished game went for its first side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Win,
    Loss,
    Draw,
    Stalled,
    /// The game crashed twice at the same decision: skipped, for a bug report.
    Crashed,
}

/// A line of `games.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameRecord {
    pub game: String,
    /// The matchup's sides, by name.
    pub a: String,
    pub b: String,
    pub pair: u32,
    pub seed: u64,
    /// The seat of `a`.
    pub seat: u8,
    /// How it went for `a`.
    pub outcome: Verdict,
    pub turns: u16,
    pub decisions: u32,
    /// For `a` and `b`: decisions searched, the iterations they ran and the engine steps
    /// they spent.
    pub searched: [u32; 2],
    pub iterations: [u64; 2],
    pub work: [u64; 2],
    /// The decision a crashed game crashed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crashed_at: Option<GameDecision>,
    /// The ladder position the game started from, by its file's name under the run's
    /// `positions/`; none for a game from the shuffle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
}

/// A line of `decisions.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub game: String,
    pub decision: GameDecision,
    pub seat: u8,
    /// The entrant, by name.
    pub agent: String,
    /// Its budget, in engine steps.
    pub budget: u64,
    pub choice: u16,
    /// The decision's index among the agent's own, and that of the decision whose search
    /// planted the trees it grew.
    pub agent_decision: AgentDecision,
    pub chain: AgentDecision,
    pub iterations: u32,
    pub work: u64,
    pub inherited: u32,
    pub visits: Vec<u32>,
    /// Each candidate's mean reward for the deciding player, averaged over the trees with each
    /// weighing the same (`Search::means`); none if never visited.
    pub mean: Vec<Option<f64>>,
    /// An anchored family's check of its choice against its default answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<AnchorRecord>,
}

/// An anchored search's check, as `decisions.jsonl` records it ([`beryl_search::AnchorCheck`]):
/// the default answer, the search's own choice, its lead in mean reward over the default's averaged
/// over the trees that visited both, and the lead's standard error across them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnchorRecord {
    /// The default answer, from greedy or the live expert according to the family.
    pub greedy: u16,
    pub search: u16,
    pub lead: f64,
    pub error: f64,
    pub trees: u16,
}

/// A budget, short: 300k, 3M, 1G.
pub fn rung_name(n: u64) -> String {
    match n {
        0 => "0".to_owned(),
        n if n % 1_000_000_000 == 0 => format!("{}G", n / 1_000_000_000),
        n if n % 1_000_000 == 0 => format!("{}M", n / 1_000_000),
        n if n % 1_000 == 0 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

/// The per-machine cache: `$BERYL_CACHE`, else `~/Library/Caches/beryl`, or on Windows
/// `%LOCALAPPDATA%\beryl`.
pub fn cache() -> PathBuf {
    match std::env::var_os("BERYL_CACHE") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ if cfg!(windows) => {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("beryl")
        }
        _ => {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Caches/beryl")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rungs_have_short_names() {
        let names: Vec<String> = [3_000, 10_000, 1_000_000, 3_000_000, 2_000_000_000, 1_500]
            .map(rung_name)
            .into();
        assert_eq!(names, ["3k", "10k", "1M", "3M", "2G", "1500"]);
    }
}
