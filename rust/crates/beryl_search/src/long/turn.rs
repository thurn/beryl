//! Propose turn continuations with the opponent passing, then test the same
//! observation-based policies in fresh worlds before changing the real move.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_engine::layout::Queue;
use beryl_engine::state::Tracker;
use beryl_engine::{SlotId, TurnStep, apply_decided, next_within, payable_responses};
use serde::{Deserialize, Serialize};

use super::{
    Agent, AgentDecision, AnchorCheck, Bitboard, Capture, Choice, Decision, DecisionKind,
    EVAL_STEPS, GameTable, ITERATION_STEPS, Layout, Leaf, LongSearch, Observation, Opt,
    ROLLOUT_TEMPERATURE, Replay, RolloutPolicy, Scratch, Search, Seat, Softmax, SplitMix64, State,
    Step, WORLD_STEPS, combat, deal, discounted, evaluate, guarded, keys, mean_and_error, observe,
    smart, value,
};
use crate::rollout::{RolloutEnd, soft_rollout_within};

const PROPOSAL_WORLDS: u64 = 24;
const SCORING_WORLDS: u16 = 48;
const WIDTH: usize = 8;

/// Deterministic work and line counts for one turn search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnReport {
    pub proposal_work: u64,
    pub scoring_work: u64,
    pub completed: u64,
    pub merged: u64,
    pub reserved: u64,
    /// Scoring lines that finished the turn with greedy after an unrepresented choice.
    pub fallbacks: u64,
    /// The root kept its default because there were too few comparable scores.
    pub defaulted: bool,
}

impl<L: Layout> LongSearch<L> {
    pub(super) fn choose_turn(
        &mut self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        decision: AgentDecision,
        capture: Option<&mut Capture<L>>,
    ) -> Choice {
        let default = self.expert.as_mut().map_or_else(
            || self.policy.choose(&self.table, obs, d),
            |expert| expert.choose(obs, d, decision),
        );
        let options = self.settings.family.options;
        let (choice, report) = search(self, obs, d, decision, default, |choice| {
            options.anchor && guarded(&self.table, &self.policy, obs, d, default, choice, options)
        });
        if let Some(capture) = capture {
            capture.finish_turn(&report);
        }
        self.last = Some(report);
        choice
    }
}

fn search<L: Layout>(
    agent: &LongSearch<L>,
    obs: Observation<'_, L>,
    d: &Decision<L>,
    decision: AgentDecision,
    default: Choice,
    guarded: impl Fn(Choice) -> bool,
) -> (Choice, Search) {
    let mut search = TurnSearch {
        table: &agent.table,
        policy: &agent.policy,
        viewer: d.player,
        turn: obs.turn().number(),
        known: L::Slot::all().filter(|&slot| obs.knows(slot)).collect(),
        scratch: Scratch::unchecked(),
        extra: 0,
        report: TurnReport::default(),
    };
    let mut seeds =
        SplitMix64::new(agent.seed ^ u64::from(decision.0).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut proposal_rng = SplitMix64::new(seeds.next_u64());
    let mut scoring_rng = SplitMix64::new(seeds.next_u64());
    let proposal_limit = agent.settings.work / 8;
    let mut roots: Vec<_> = (0..d.len()).map(|i| Choice(i as u16)).collect();
    roots.sort_by_key(|&c| (c != default, d.candidates.option(c) != Some(Opt::Pass)));
    let learned = search.learn(
        obs,
        d,
        &roots,
        agent.settings.family.sight,
        &mut proposal_rng,
        proposal_limit,
    );
    search.report.proposal_work = search.spent();

    let mut rewards = vec![Vec::new(); d.len()];
    if learned[usize::from(default.0)].is_some() {
        roots.retain(|c| learned[usize::from(c.0)].is_some());
        if roots.len() > 1 {
            search.score_worlds(
                agent,
                obs,
                d,
                &roots,
                &learned,
                &mut rewards,
                &mut scoring_rng,
            );
        }
    }

    let mean = |xs: &Vec<f64>| (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64);
    let means: Vec<_> = rewards.iter().map(mean).collect();
    let best = roots
        .iter()
        .copied()
        .filter(|c| means[usize::from(c.0)].is_some())
        .reduce(|a, b| {
            if means[usize::from(a.0)] >= means[usize::from(b.0)] {
                a
            } else {
                b
            }
        })
        .unwrap_or(default);
    let comparable = rewards[usize::from(default.0)].len() == usize::from(SCORING_WORLDS);
    let anchor = (comparable && guarded(best)).then(|| {
        let leads: Vec<_> = rewards[usize::from(best.0)]
            .iter()
            .zip(&rewards[usize::from(default.0)])
            .map(|(best, base)| best - base)
            .collect();
        let (lead, error) = mean_and_error(&leads);
        AnchorCheck {
            greedy: default,
            search: best,
            lead,
            error,
            trees: leads.len() as u16,
        }
    });
    let visits: Vec<_> = rewards.iter().map(|xs| xs.len() as u32).collect();
    let choice = if comparable {
        anchor.map_or(best, |a| a.played())
    } else {
        search.report.defaulted = true;
        default
    };
    search.report.scoring_work = search.spent() - search.report.proposal_work;
    let summary = Search {
        decision,
        chain: decision,
        rollouts: visits.iter().sum(),
        inherited: 0,
        work: search.spent(),
        visits,
        rewards: rewards.iter().map(|xs| xs.iter().sum()).collect(),
        means,
        anchor,
        turn: Some(search.report),
    };
    (choice, summary)
}

/// Observations include the offered actions, including duplicate ordinals, but
/// omit the private deal, library order of unknown cards, and the RNG.
fn observation_key<L: Layout>(obs: Observation<'_, L>, d: &Decision<L>, actions: &[u64]) -> u64 {
    let mut h = DefaultHasher::new();
    obs.turn().hash(&mut h);
    obs.stack().hash(&mut h);
    obs.triggers().hash(&mut h);
    obs.effects().hash(&mut h);
    obs.battlefield().hash(&mut h);
    for seat in Seat::BOTH {
        obs.player(seat).hash(&mut h);
        obs.graveyard(seat).hash(&mut h);
        for &slot in obs.library(seat) {
            // Which known cards occupy which positions matters. So does the
            // number of cards between them.
            obs.knows(slot).then_some(slot).hash(&mut h);
        }
    }
    for slot in L::Slot::all() {
        obs.knows(slot).hash(&mut h);
        obs.object(slot).hash(&mut h);
    }
    for &slot in obs.battlefield() {
        obs.derived(slot).hash(&mut h);
        for memo in obs.memos(slot).iter() {
            memo.hash(&mut h);
        }
        for tracker in Tracker::ALL {
            obs.tracked(tracker, slot).hash(&mut h);
        }
    }
    d.player.hash(&mut h);
    d.kind.hash(&mut h);
    d.prompt.hash(&mut h);
    d.about.hash(&mut h);
    // Action order is visible, and some picks (such as payments) use indices.
    actions.hash(&mut h);
    h.finish()
}

type Continuation = HashMap<u64, u64>;

/// Each proposal contributes votes for observations encountered in its complete
/// lines. The held-out worlds all replay this one policy per root choice.
#[derive(Default)]
struct Votes {
    observations: HashMap<u64, Vec<(u64, u32)>>,
    complete: bool,
}

impl Votes {
    fn add(&mut self, path: &[(u64, u64)]) {
        self.complete = true;
        for &(obs, action) in path {
            let votes = self.observations.entry(obs).or_default();
            if let Some((_, count)) = votes.iter_mut().find(|(a, _)| *a == action) {
                *count += 1;
            } else {
                votes.push((action, 1));
            }
        }
    }

    fn policy(self) -> Option<Continuation> {
        self.complete.then(|| {
            self.observations
                .into_iter()
                .map(|(obs, votes)| {
                    let (action, _) = votes
                        .into_iter()
                        .max_by_key(|&(action, count)| (count, action))
                        .expect("at least one vote");
                    (obs, action)
                })
                .collect()
        })
    }
}

struct TurnSearch<'a, L: Layout> {
    table: &'a GameTable,
    policy: &'a RolloutPolicy,
    viewer: Seat,
    turn: u16,
    known: Bitboard<L::Slot>,
    scratch: Scratch<L>,
    extra: u64,
    report: TurnReport,
}

#[derive(Clone)]
struct Line<L: Layout> {
    state: Box<State<L>>,
    combat: [combat::Plan<L>; 2],
    path: Vec<(u64, u64)>,
    /// Sum of differences from the highest rollout policy score at each pick.
    regret: i64,
    /// The end position's value in this proposal world, or zero for a partial line.
    estimate: i32,
    responses: Vec<Opt<L>>,
}

impl<L: Layout> Line<L> {
    fn new(state: State<L>) -> Self {
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

    fn reserves_response(&self) -> bool {
        !self.responses.is_empty()
    }
}

/// Keeps a beam's best lines in policy order. Leaf beams prefer a payable
/// response as a tie-breaker and reserve a slot for one even when it ranks low.
struct Beam<L: Layout> {
    lines: Vec<(u64, Line<L>)>,
}

impl<L: Layout> Default for Beam<L> {
    fn default() -> Self {
        Self { lines: Vec::new() }
    }
}

impl<L: Layout> Beam<L> {
    fn push(&mut self, line: Line<L>, report: &mut TurnReport) {
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

impl<L: Layout> TurnSearch<'_, L> {
    fn learn(
        &mut self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        roots: &[Choice],
        sight: crate::Sight,
        rng: &mut SplitMix64,
        limit: u64,
    ) -> Vec<Option<Continuation>> {
        let mut votes: Vec<Votes> = (0..d.len()).map(|_| Votes::default()).collect();
        let allowance = (limit / PROPOSAL_WORLDS) / roots.len() as u64;
        for _ in 0..PROPOSAL_WORLDS {
            if !self.room(limit, WORLD_STEPS + 1) {
                break;
            }
            let world = self.deal(obs, sight, rng);
            for &first in roots {
                let cap = limit.min(self.spent() + allowance);
                let leaves = self.propose(&world, d, first, cap);
                let reserve = leaves.iter().find(|line| line.reserves_response());
                self.report.reserved += u64::from(reserve.is_some());
                let best = if d.candidates.option(first) == Some(Opt::Pass) {
                    reserve.or_else(|| leaves.first())
                } else {
                    leaves.first()
                };
                if let Some(best) = best {
                    votes[usize::from(first.0)].add(&best.path);
                }
            }
        }
        votes.into_iter().map(Votes::policy).collect()
    }

    fn deal(
        &mut self,
        obs: Observation<'_, L>,
        sight: crate::Sight,
        rng: &mut SplitMix64,
    ) -> State<L> {
        self.scratch.counts.copied();
        self.extra += WORLD_STEPS;
        deal(self.table, sight, obs, rng)
    }

    fn spent(&self) -> u64 {
        self.scratch.counts.decision_work() + self.extra
    }

    fn engine_limit(&self, limit: u64) -> u64 {
        limit.saturating_sub(self.extra)
    }

    fn room(&self, limit: u64, work: u64) -> bool {
        limit.saturating_sub(self.spent()) >= work
    }

    fn copy(&mut self, line: &Line<L>) -> Line<L> {
        self.scratch.counts.copied();
        line.clone()
    }

    fn advance(&mut self, line: &mut Line<L>, limit: u64) -> Option<Step<L>> {
        let limit = self.engine_limit(limit);
        next_within(
            &mut line.state,
            self.table,
            &mut self.scratch,
            &mut Replay,
            (self.turn.saturating_add(1), TurnStep::Untap),
            limit,
        )
    }

    fn apply(&mut self, line: &mut Line<L>, d: &Decision<L>, c: Choice) {
        apply_decided(
            &mut line.state,
            self.table,
            &mut self.scratch,
            &mut Replay,
            d,
            c,
        );
    }

    fn start(
        &mut self,
        world: &State<L>,
        d: &Decision<L>,
        first: Choice,
        limit: u64,
    ) -> Option<Line<L>> {
        if !self.room(limit, 3 + ITERATION_STEPS) {
            return None;
        }
        self.scratch.counts.copied();
        self.extra += ITERATION_STEPS;
        let mut line = Line::new(world.clone());
        self.apply(&mut line, d, first);
        smart(&mut line.state);
        Some(line)
    }

    /// Pass priority for the opponent and play required opponent choices from
    /// their own view. Declarations use a combat plan based on the declaring
    /// player's observation, even when a proposed world has extra information.
    fn passes(&self, line: &Line<L>, d: &Decision<L>) -> bool {
        d.kind == DecisionKind::Priority
            && (d.player != self.viewer
                || line
                    .state
                    .stack()
                    .last()
                    .is_some_and(|e| e.controller() == d.player))
    }

    fn forced(&mut self, line: &mut Line<L>, d: &Decision<L>, limit: u64) -> Option<Choice> {
        if self.passes(line, d) {
            return Some(Choice(0));
        }
        if combat::handles(d.kind) {
            if !self.room(limit, WORLD_STEPS + 1) {
                return None;
            }
            let obs = Observation::new(&line.state, d.player);
            let mut rng = SplitMix64::new(u64::from(self.turn));
            let world = obs.determinize(self.table, &mut rng);
            self.scratch.counts.copied();
            self.extra += WORLD_STEPS;
            let limit = self.engine_limit(limit);
            return combat::Sim::<_, combat::General>::new(
                self.table,
                self.policy,
                &mut self.scratch,
                d.player,
                &mut line.combat[d.player.index()],
            )
            .choose_within(&world, d, limit);
        }
        let obs = observe(&line.state, self.table, &mut self.scratch, d.player);
        Some(self.policy.choose(self.table, obs, d))
    }

    fn propose(
        &mut self,
        world: &State<L>,
        d: &Decision<L>,
        first: Choice,
        limit: u64,
    ) -> Vec<Line<L>> {
        let initial = self.spent();
        let allowance = limit - initial;
        let mut leaves = Beam::default();
        // Keep one pass line and one greedy line ahead of beam pruning.
        for (pass, divisor) in [(true, 4), (false, 2)] {
            let reserved = initial + allowance / divisor;
            if let Some(mut line) = self.start(world, d, first, reserved)
                && self.finish(&mut line, None, pass, reserved)
            {
                self.complete(line, &mut leaves, reserved);
            }
        }
        let Some(line) = self.start(world, d, first, limit) else {
            return leaves.lines.into_iter().map(|(_, line)| line).collect();
        };
        let mut beam = Beam::default();
        beam.push(line, &mut self.report);
        while !beam.lines.is_empty() && self.room(limit, 3) {
            let mut next = Beam::default();
            for (_, mut line) in beam.lines {
                let step = self.advance(&mut line, limit);
                match step {
                    Some(Step::Decide(d)) => self.expand(line, &d, &mut next, limit),
                    Some(Step::Over(_)) => self.complete(line, &mut leaves, limit),
                    None if self.finished(&line) => self.complete(line, &mut leaves, limit),
                    None => break,
                }
            }
            beam = next;
        }
        leaves.lines.into_iter().map(|(_, line)| line).collect()
    }

    fn expand(&mut self, mut line: Line<L>, d: &Decision<L>, next: &mut Beam<L>, limit: u64) {
        if !self.room(limit, 4) {
            return;
        }
        if d.player != self.viewer || combat::handles(d.kind) || self.passes(&line, d) {
            if let Some(c) = self.forced(&mut line, d, limit)
                && self.room(limit, 2)
            {
                self.apply(&mut line, d, c);
                next.push(line, &mut self.report);
            }
            return;
        }
        let shown = observe(&line.state, self.table, &mut self.scratch, d.player);
        let actions = Self::actions(self.known, shown, d);
        let obs = observation_key(shown, d, &actions);
        let scores = self.policy.scores(self.table, shown, d);
        let best = scores.iter().copied().flatten().max().unwrap_or(0);
        let mut order: Vec<_> = (0..d.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(scores[i]));
        for i in order.into_iter().take(WIDTH) {
            if !self.room(limit, 3) {
                break;
            }
            let mut child = self.copy(&line);
            child.regret += scores[i].map_or(10_000, |s| i64::from(best) - i64::from(s));
            child.path.push((obs, actions[i]));
            self.apply(&mut child, d, Choice(i as u16));
            next.push(child, &mut self.report);
        }
    }

    fn complete(&mut self, mut line: Line<L>, leaves: &mut Beam<L>, limit: u64) {
        self.report.completed += 1;
        if !self.room(limit, EVAL_STEPS) {
            return;
        }
        self.extra += EVAL_STEPS;
        let p0 = line.state.outcome().map_or_else(
            || evaluate(&line.state, self.policy.values()),
            |outcome| value(outcome, 0),
        );
        let chance = if self.viewer == Seat::P0 {
            p0
        } else {
            1.0 - p0
        };
        line.estimate = (10_000.0 * chance) as i32;
        if line.state.outcome().is_none()
            && line.state.turn().active() != self.viewer
            && self.room(limit, EVAL_STEPS)
        {
            self.extra += EVAL_STEPS;
            let obs = Observation::new(&line.state, self.viewer);
            if let Ok(options) = payable_responses(&line.state, self.table, self.viewer) {
                line.responses
                    .extend(options.iter().copied().filter(|&opt| {
                        opt.object_named(&line.state)
                            .is_some_and(|slot| obs.knows(slot))
                    }));
            }
        }
        leaves.push(line, &mut self.report);
    }

    fn finished(&self, line: &Line<L>) -> bool {
        line.state.turn().number() > self.turn || line.state.outcome().is_some()
    }

    fn actions(known: Bitboard<L::Slot>, obs: Observation<'_, L>, d: &Decision<L>) -> Vec<u64> {
        // A newly revealed card keys by identity. For a still-hidden card, use
        // the slot even when taking the action itself would reveal the card.
        let hidden: Bitboard<L::Slot> = L::Slot::all().filter(|&slot| !obs.knows(slot)).collect();
        keys(obs.shown(), d, known | hidden, obs.viewer())
    }

    /// Replays a fixed continuation. At an unrepresented observation the
    /// rollout policy supplies the rest of the turn's choices.
    fn finish(
        &mut self,
        line: &mut Line<L>,
        policy: Option<&Continuation>,
        pass: bool,
        limit: u64,
    ) -> bool {
        let mut policy = policy;
        let proposing = policy.is_none();
        while let Some(Step::Decide(d)) = self.advance(line, limit) {
            if !self.room(limit, 4) {
                return false;
            }
            let ours =
                d.player == self.viewer && !combat::handles(d.kind) && !self.passes(line, &d);
            let seen = (ours && (proposing || policy.is_some())).then(|| {
                let obs = observe(&line.state, self.table, &mut self.scratch, d.player);
                let actions = Self::actions(self.known, obs, &d);
                let key = observation_key(obs, &d, &actions);
                let scores = proposing.then(|| self.policy.scores(self.table, obs, &d));
                (key, actions, scores)
            });
            let planned = seen.as_ref().and_then(|(obs, actions, _)| {
                let want = policy?.get(obs)?;
                actions
                    .iter()
                    .position(|a| a == want)
                    .map(|i| Choice(i as u16))
            });
            if ours && planned.is_none() && policy.take().is_some() {
                self.report.fallbacks += 1;
            }
            let choice = if pass && d.kind == DecisionKind::Priority {
                Some(Choice(0))
            } else {
                planned.or_else(|| self.forced(line, &d, limit))
            };
            let Some(choice) = choice.filter(|_| self.room(limit, 2)) else {
                return false;
            };
            if let Some((obs, actions, Some(scores))) = seen {
                let i = usize::from(choice.0);
                let best = scores.iter().copied().flatten().max().unwrap_or(0);
                line.regret += scores[i].map_or(10_000, |s| i64::from(best) - i64::from(s));
                line.path.push((obs, actions[i]));
            }
            if !self.room(limit, 2) {
                return false;
            }
            self.apply(line, &d, choice);
        }
        self.finished(line)
    }

    #[allow(clippy::too_many_arguments)]
    fn score_worlds(
        &mut self,
        agent: &LongSearch<L>,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        roots: &[Choice],
        learned: &[Option<Continuation>],
        rewards: &mut [Vec<f64>],
        rng: &mut SplitMix64,
    ) {
        let limit = agent.settings.work;
        for _ in 0..SCORING_WORLDS {
            if !self.room(limit, WORLD_STEPS + 1) {
                break;
            }
            let world = self.deal(obs, agent.settings.family.sight, rng);
            let seed = rng.next_u64();
            let mut round = Vec::with_capacity(roots.len());
            for &first in roots {
                let i = usize::from(first.0);
                let Some(mut line) = self.start(&world, d, first, limit) else {
                    return;
                };
                if !self.finish(&mut line, learned[i].as_ref(), false, limit) {
                    return;
                }
                let Some(reward) = self.reward(&mut line, agent.settings.family.leaf, seed, limit)
                else {
                    return;
                };
                crate::mcts::count_rollout();
                round.push((i, reward));
            }
            for (i, reward) in round {
                rewards[i].push(reward);
            }
        }
    }

    fn reward(&mut self, line: &mut Line<L>, leaf: Leaf, seed: u64, limit: u64) -> Option<f64> {
        let s = &mut line.state;
        let outcome = if let Some(outcome) = s.outcome() {
            Some(outcome)
        } else if leaf == Leaf::Eval {
            None
        } else {
            let mut rng = SplitMix64::new(seed);
            s.reseed(rng.next_u64());
            let mut soft = Softmax {
                rng: &mut rng,
                temperature: ROLLOUT_TEMPERATURE,
            };
            let cutoff = match leaf {
                Leaf::Cutoff { turns } => s.turn().number().saturating_add(u16::from(turns)),
                _ => u16::MAX,
            };
            let limit = self.engine_limit(limit);
            match soft_rollout_within(
                s,
                self.table,
                &mut self.scratch,
                self.policy,
                &mut soft,
                cutoff,
                limit,
            )? {
                RolloutEnd::Outcome(outcome) => Some(outcome),
                RolloutEnd::Cutoff => None,
            }
        };
        let turns = s.turn().number().saturating_sub(self.turn);
        let reward = if let Some(outcome) = outcome {
            value(outcome, turns)
        } else {
            if !self.room(limit, EVAL_STEPS) {
                return None;
            }
            self.extra += EVAL_STEPS;
            discounted(evaluate(s, self.policy.values()), turns)
        };
        Some(if self.viewer == Seat::P0 {
            reward
        } else {
            1.0 - reward
        })
    }
}

#[cfg(test)]
mod tests;
