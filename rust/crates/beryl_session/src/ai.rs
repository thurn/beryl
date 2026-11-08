//! The agent's decisions as a game plays, for an AI-play bug report ([`crate::bugs`]) and the
//! journal: each one's candidates as the UI names them, what it chose, how long it thought,
//! and the search behind it (the root visits and mean reward of each candidate, summed over
//! its trees), as the agent itself ran it. `just review --agent recorded` searches a replay's
//! decisions again, but its searches are a fresh sample; these are the game's own.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};

use beryl_search::Search;
use serde::Serialize;

use crate::protocol::{Kind, Step};

/// How many of the agent's latest decisions a game keeps.
pub const KEPT: usize = 200;

/// The agent's latest decisions, oldest first, shared between a game and its session.
pub type AgentMoves = Arc<Mutex<VecDeque<AgentMove>>>;

/// One of the agent's decisions.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMove {
    /// Where its answer stands in the replay's choice log.
    pub log: usize,
    pub turn: u16,
    pub step: Step,
    pub kind: Kind,
    /// Each candidate, as the UI would name it to the agent's seat.
    pub candidates: Vec<String>,
    pub chosen: u16,
    /// How long it thought, in milliseconds.
    pub ms: u64,
    /// None for an answer made without a search: the agent does not search, or the decision
    /// had one sensible answer.
    pub search: Option<SearchView>,
}

/// A search, as [`Search`] records it, with each candidate's mean.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchView {
    /// The decision's index among the agent's own, and the one whose fresh search planted the
    /// trees it grew (itself, unless it reused them).
    pub decision: u32,
    pub chain: u32,
    pub rollouts: u32,
    /// Root visits brought from the decision before.
    pub inherited: u32,
    /// Engine steps spent.
    pub work: u64,
    /// Root visits per candidate, summed across the trees.
    pub visits: Vec<u32>,
    /// Mean reward per candidate for the agent: none for one never visited.
    pub means: Vec<Option<f64>>,
    /// An anchored search's check of its choice against greedy's answer.
    pub anchor: Option<String>,
}

impl SearchView {
    pub(crate) fn of(s: &Search) -> SearchView {
        SearchView {
            decision: s.decision.0,
            chain: s.chain.0,
            rollouts: s.rollouts,
            inherited: s.inherited,
            work: s.work,
            visits: s.visits.clone(),
            means: s
                .visits
                .iter()
                .zip(&s.rewards)
                .map(|(&v, &r)| (v > 0).then(|| r / f64::from(v)))
                .collect(),
            anchor: s.anchor.as_ref().map(|a| format!("{a:?}")),
        }
    }
}

/// Adds `m` to `moves`, forgetting the oldest past [`KEPT`].
pub(crate) fn keep(moves: &AgentMoves, m: AgentMove) {
    let mut moves = moves.lock().unwrap_or_else(PoisonError::into_inner);
    if moves.len() == KEPT {
        moves.pop_front();
    }
    moves.push_back(m);
}

/// The latest of `moves`, oldest first.
pub fn latest(moves: &AgentMoves) -> Vec<AgentMove> {
    moves
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .cloned()
        .collect()
}

impl AgentMove {
    /// The decision in a few lines, as `just review` prints one: the candidates, the choice,
    /// and each candidate's root visits and mean, the chosen one marked.
    pub fn describe(&self) -> String {
        let step = serde_json::to_value(self.step)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let kind = serde_json::to_value(self.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let chosen = self
            .candidates
            .get(usize::from(self.chosen))
            .map_or("?", String::as_str);
        let mut out = format!(
            "Turn {} {step}, {kind} (log {}): {chosen}, in {} ms",
            self.turn, self.log, self.ms
        );
        let Some(s) = &self.search else {
            out.push_str(", no search\n");
            return out;
        };
        writeln!(
            out,
            ", {} rollouts{}",
            s.rollouts,
            if s.chain == s.decision {
                String::new()
            } else {
                format!(", trees from its decision {}", s.chain)
            }
        )
        .expect("a String takes writes");
        for (i, name) in self.candidates.iter().enumerate() {
            let visits = s.visits.get(i).copied().unwrap_or(0);
            let mean = s
                .means
                .get(i)
                .copied()
                .flatten()
                .map_or("-".to_owned(), |m| format!("{m:.3}"));
            let mark = if i == usize::from(self.chosen) {
                "*"
            } else {
                " "
            };
            writeln!(out, "  {mark}[{i}] {visits:>7} {mean:>6}  {name}")
                .expect("a String takes writes");
        }
        if let Some(anchor) = &s.anchor {
            writeln!(out, "  anchor: {anchor}").expect("a String takes writes");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_view_has_each_candidates_mean() {
        let search = Search {
            decision: beryl_search::AgentDecision(4),
            chain: beryl_search::AgentDecision(2),
            rollouts: 90,
            inherited: 10,
            work: 1000,
            visits: vec![60, 40, 0],
            rewards: vec![30.0, 30.0, 0.0],
            anchor: None,
        };
        let view = SearchView::of(&search);
        assert_eq!(view.means, vec![Some(0.5), Some(0.75), None]);
        assert_eq!((view.decision, view.chain), (4, 2));
    }

    #[test]
    fn describes_a_search_with_the_choice_marked() {
        let m = AgentMove {
            log: 12,
            turn: 7,
            step: Step::PrecombatMain,
            kind: Kind::Priority,
            candidates: vec!["Pass".to_owned(), "Cast Reprieve".to_owned()],
            chosen: 1,
            ms: 3000,
            search: Some(SearchView {
                decision: 5,
                chain: 5,
                rollouts: 100,
                inherited: 0,
                work: 1,
                visits: vec![40, 60],
                means: vec![Some(0.5), Some(0.61)],
                anchor: None,
            }),
        };
        let text = m.describe();
        assert!(text.starts_with("Turn 7 precombatMain, priority (log 12): Cast Reprieve"));
        assert!(text.contains(" *[1]      60  0.610  Cast Reprieve"));
    }
}
