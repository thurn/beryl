//! Replaying a turn-search proposal or scoring line in [`Large`] when a step or rollout runs
//! out of [`Small`](beryl_engine::Small)'s room ([`crate::wide`]).

use beryl_engine::{Large, Layout, SlotId};

use crate::wide::{can_widen, charge, ran_out, widened};

use super::{
    Choice, Continuation, Decision, Leaf, Line, Scratch, State, Step, TurnReport, TurnSearch,
};

/// Outcome of proposing continuations for one root choice in one world.
pub(super) enum Propose<L: Layout> {
    Leaves(Vec<Line<L>>),
    /// A line ran out of `Small`'s room; replay the proposal for this root choice in `Large`.
    RanOut,
}

/// The chosen continuation path and response-reservation flag from a `Large` proposal replay.
pub(super) struct WideProposal {
    pub(super) path: Option<Vec<(u64, u64)>>,
    pub(super) reserved: bool,
}

/// The choices taken by a scoring line during [`TurnSearch::finish`], stored on the stack so
/// the common `Small` path allocates nothing. If a step or the scoring rollout runs out of
/// `Small`'s room, replaying these choices in `Large` reaches the exact point where `Small`
/// stopped and goes on past it.
pub(super) struct Choices {
    buf: [Choice; 256],
    len: usize,
    overflow: Vec<Choice>,
}

impl Choices {
    pub(super) fn new() -> Self {
        Self {
            buf: [Choice(0); 256],
            len: 0,
            overflow: Vec::new(),
        }
    }

    #[inline]
    pub(super) fn push(&mut self, c: Choice) {
        if self.len < self.buf.len() {
            self.buf[self.len] = c;
            self.len += 1;
        } else {
            self.overflow.push(c);
        }
    }

    fn iter(&self) -> impl Iterator<Item = Choice> + '_ {
        self.buf[..self.len]
            .iter()
            .copied()
            .chain(self.overflow.iter().copied())
    }
}

impl<'a, L: Layout> TurnSearch<'a, L> {
    /// Builds a fresh `TurnSearch<Large>` with the same root viewer, scoring, turn and
    /// known slots.
    fn wide_search(&self) -> TurnSearch<'a, Large> {
        let known = L::Slot::all()
            .filter(|&slot| self.known.contains(slot))
            .map(|slot| <Large as Layout>::Slot::from_index(slot.index()))
            .collect();
        TurnSearch {
            table: self.table,
            policy: self.policy,
            viewer: self.viewer,
            bonus: self.bonus,
            turn: self.turn,
            known,
            scratch: Scratch::unchecked(),
            extra: 0,
            report: TurnReport::default(),
        }
    }

    /// Charges work and counts from a `Large` replay back to this search.
    fn charge_wide(&mut self, wide: &TurnSearch<'_, Large>) {
        charge(&mut self.scratch, &wide.scratch);
        self.extra += wide.extra;
        self.report.completed += wide.report.completed;
        self.report.merged += wide.report.merged;
        self.report.reserved += wide.report.reserved;
        self.report.fallbacks += wide.report.fallbacks;
    }

    /// Replays [`TurnSearch::propose`] for `first` in `Large` from `world` widened, returning
    /// the chosen continuation path (if any leaf completed) and whether a response was reserved.
    #[cold]
    #[inline(never)]
    pub(super) fn propose_wide(
        &mut self,
        world: &State<L>,
        first: Choice,
        prefer_reserve: bool,
        limit: u64,
    ) -> Option<WideProposal> {
        let world = widened(world)?;
        let mut wide = self.wide_search();
        let remaining = limit.saturating_sub(self.spent());
        let leaves = match wide.propose(&world, None, first, remaining) {
            Propose::Leaves(leaves) => leaves,
            Propose::RanOut => unreachable!("Large never widens"),
        };
        self.charge_wide(&wide);
        let reserve = leaves.iter().find(|line| line.reserves_response());
        let reserved = reserve.is_some();
        let best = if prefer_reserve {
            reserve.or_else(|| leaves.first())
        } else {
            leaves.first()
        };
        Some(WideProposal {
            path: best.map(|line| line.path.clone()),
            reserved,
        })
    }

    /// Plays and scores one root choice in `L`, falling back to [`Self::score_wide`] if any
    /// step of the turn or the scoring rollout runs out of `Small`'s room.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn score_line(
        &mut self,
        world: &State<L>,
        d: &Decision<L>,
        first: Choice,
        policy: Option<&Continuation>,
        leaf: Leaf,
        seed: u64,
        limit: u64,
    ) -> Option<f64> {
        let mut line = self.start(world, Some(d), first, limit)?;
        if can_widen::<L>() && ran_out(&line.state) {
            return self.score_wide(world, first, None, None, leaf, seed, limit);
        }
        let mut choices = Choices::new();
        let finished = self.finish(&mut line, policy, false, limit, Some(&mut choices));
        if can_widen::<L>() && ran_out(&line.state) {
            return self.score_wide(world, first, None, Some(&choices), leaf, seed, limit);
        }
        if !finished {
            return None;
        }
        let reward = self.reward(&mut line, leaf, seed, limit);
        if can_widen::<L>() && ran_out(&line.state) {
            return self.score_wide(world, first, None, Some(&choices), leaf, seed, limit);
        }
        reward
    }

    /// Plays and scores one root choice in `Large` from `world` widened. When replaying a
    /// `Small` line that stalled mid-turn or in its scoring rollout, `prefix` holds the choices
    /// already applied in `Small` so the replay reaches that point before continuing in `Large`.
    #[cold]
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn score_wide(
        &mut self,
        world: &State<L>,
        first: Choice,
        policy: Option<&Continuation>,
        prefix: Option<&Choices>,
        leaf: Leaf,
        seed: u64,
        limit: u64,
    ) -> Option<f64> {
        let world = widened(world)?;
        let mut wide = self.wide_search();
        let remaining = limit.saturating_sub(self.spent());
        let reward = wide.play_wide(&world, first, policy, prefix, leaf, seed, remaining);
        self.charge_wide(&wide);
        reward
    }
}

impl TurnSearch<'_, Large> {
    #[allow(clippy::too_many_arguments)]
    fn play_wide(
        &mut self,
        world: &State<Large>,
        first: Choice,
        policy: Option<&Continuation>,
        prefix: Option<&Choices>,
        leaf: Leaf,
        seed: u64,
        limit: u64,
    ) -> Option<f64> {
        let mut line = self.start(world, None, first, limit)?;
        if let Some(choices) = prefix {
            for c in choices.iter() {
                if !self.room(limit, 4) {
                    return None;
                }
                match self.advance(&mut line, limit) {
                    Some(Step::Decide(d)) => {
                        if !self.room(limit, 2) {
                            return None;
                        }
                        self.apply(&mut line, &d, c);
                    }
                    Some(Step::Over(_)) => break,
                    None => {
                        if self.finished(&line) {
                            break;
                        }
                        return None;
                    }
                }
            }
        }
        if !self.finish(&mut line, policy, false, limit, None) {
            return None;
        }
        self.reward(&mut line, leaf, seed, limit)
    }
}
