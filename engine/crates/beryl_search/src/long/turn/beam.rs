//! Lines and beams for the turn search's proposal phase.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::{Layout, Opt, State, TurnReport, WIDTH, combat};

#[derive(Clone)]
pub(super) struct Line<L: Layout> {
    pub(super) state: Box<State<L>>,
    pub(super) combat: [combat::Plan<L>; 2],
    pub(super) path: Vec<(u64, u64)>,
    /// Sum of differences from the highest rollout policy score at each pick.
    pub(super) regret: i64,
    /// The end position's value in this proposal world, or zero for a partial line.
    pub(super) estimate: i32,
    pub(super) responses: Vec<Opt<L>>,
}

impl<L: Layout> Line<L> {
    pub(super) fn new(state: State<L>) -> Self {
        Self {
            state: Box::new(state),
            combat: Default::default(),
            path: Vec::new(),
            regret: 0,
            estimate: 0,
            responses: Vec::new(),
        }
    }

    fn state_hash(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.state.hash(&mut h);
        h.finish()
    }

    fn can_merge(&self) -> bool {
        self.combat
            .iter()
            .all(|plan| !plan.is_pending(self.state.turn().number()))
    }

    pub(super) fn reserves_response(&self) -> bool {
        !self.responses.is_empty()
    }
}

/// Keeps a beam's best lines in policy order. Leaf beams prefer a payable
/// response as a tie-breaker and reserve a slot for one even when it ranks low.
pub(super) struct Beam<L: Layout> {
    pub(super) lines: Vec<(u64, Line<L>)>,
}

impl<L: Layout> Default for Beam<L> {
    fn default() -> Self {
        Self { lines: Vec::new() }
    }
}

impl<L: Layout> Beam<L> {
    pub(super) fn push(&mut self, line: Line<L>, report: &mut TurnReport) {
        let key = line.state_hash();
        let rank = |line: &Line<L>| {
            (
                std::cmp::Reverse(line.estimate),
                line.regret,
                std::cmp::Reverse(line.responses.len()),
            )
        };
        if line.can_merge()
            && let Some(i) = self
                .lines
                .iter()
                .position(|(hash, old)| *hash == key && old.can_merge() && old.state == line.state)
        {
            report.merged += 1;
            if rank(&self.lines[i].1) <= rank(&line) {
                return;
            }
            self.lines.remove(i);
        }
        let i = self
            .lines
            .partition_point(|(_, old)| rank(old) <= rank(&line));
        self.lines.insert(i, (key, line));
        if self.lines.len() > WIDTH {
            let last = self.lines.len() - 1;
            if self.lines[last].1.reserves_response()
                && !self.lines[..last]
                    .iter()
                    .any(|(_, line)| line.reserves_response())
            {
                self.lines.remove(last - 1);
            } else {
                self.lines.pop();
            }
        }
    }
}
