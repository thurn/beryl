//! Propose turn continuations with the opponent passing, then test the same
//! observation-based policies in fresh worlds before changing the real move.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_engine::layout::Queue;
use beryl_engine::state::{Count, Tracker};
use beryl_engine::{AdvanceUntil, SlotId, apply, apply_decided, next_within, payable_responses};
use serde::{Deserialize, Serialize};

use super::{
    Agent, AgentDecision, AnchorCheck, Bitboard, Capture, Choice, Decision, DecisionKind,
    EVAL_STEPS, GameTable, ITERATION_STEPS, Layout, Leaf, LongSearch, Method, Observation, Opt,
    ROLLOUT_TEMPERATURE, Replay, RolloutPolicy, Scratch, Search, Seat, Softmax, SplitMix64, State,
    Step, WORLD_STEPS, combat, deal, discounted, evaluate, guarded, keys, mean_and_error, observe,
    smart, value,
};
use crate::rollout::{RolloutEnd, soft_rollout_within};
use crate::wide::{can_widen, ran_out};

mod beam;
mod race;
mod wide;
use beam::{Beam, Line};
use race::{RACE_INTERVAL, best, race_out};
use wide::{Choices, Propose};

const PROPOSAL_WORLDS: u64 = 24;
const MIN_SCORING_WORLDS: u16 = 48;
const MAX_SCORING_WORLDS: u16 = 4_096;
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
    /// Root moves dropped from scoring by [`super::Options::race`].
    #[serde(default)]
    pub dropped: u64,
    /// Scoring worlds in which every root still in the race was scored.
    #[serde(default)]
    pub worlds: u64,
}

impl<L: Layout> LongSearch<L> {
    pub(super) fn choose_turn(
        &mut self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        decision: AgentDecision,
        capture: Option<&mut Capture<L>>,
    ) -> Choice {
        let root = self.root_policy.as_ref().unwrap_or(&self.policy);
        let default = self.expert.as_mut().map_or_else(
            || root.choose(&self.table, obs, d),
            |expert| expert.choose(obs, d, decision),
        );
        let options = self.settings.family.options;
        let (choice, report) = search(self, obs, d, decision, default, |choice| {
            options.anchor && guarded(&self.table, self.root(), obs, d, default, choice, options)
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
        viewer: d.decider,
        bonus: agent.settings.family.options.bonus,
        turn: obs.shown().elapsed_turns(),
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
    let (learned, wide_roots) = search.learn(
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
                &wide_roots,
                &mut rewards,
                &mut scoring_rng,
            );
        }
    }

    let mean = |xs: &Vec<f64>| (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64);
    let means: Vec<_> = rewards.iter().map(mean).collect();
    let best = best(&roots, &rewards, &means, default);
    let comparable = rewards[usize::from(default.0)].len() >= usize::from(MIN_SCORING_WORLDS);
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
            // Each scoring world uses the same rollout seed for every continuation.
            pairs: agent
                .settings
                .family
                .options
                .first
                .then_some(leads.len() as u64),
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
        for count in Count::ALL {
            obs.counted(count, seat).hash(&mut h);
        }
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
    /// Whether speed is only a bonus in the scores ([`super::Options::bonus`]).
    bonus: bool,
    /// The turns started across the playthrough at the root ([`State::elapsed_turns`]): a
    /// turn's number starts again in a restarted game and in a subgame, and these do not.
    turn: u16,
    known: Bitboard<L::Slot>,
    scratch: Scratch<L>,
    extra: u64,
    report: TurnReport,
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
    ) -> (Vec<Option<Continuation>>, Vec<bool>) {
        let mut votes: Vec<Votes> = (0..d.len()).map(|_| Votes::default()).collect();
        let mut wide_roots = vec![false; d.len()];
        let allowance = (limit / PROPOSAL_WORLDS) / roots.len() as u64;
        for _ in 0..PROPOSAL_WORLDS {
            if !self.room(limit, WORLD_STEPS + 1) {
                break;
            }
            let world = self.deal(obs, sight, rng);
            for &first in roots {
                let i = usize::from(first.0);
                let cap = limit.min(self.spent() + allowance);
                let prefer_reserve = d.candidates.option(first) == Some(Opt::Pass);
                if wide_roots[i] {
                    if let Some(proposed) = self.propose_wide(&world, first, prefer_reserve, cap) {
                        self.report.reserved += u64::from(proposed.reserved);
                        if let Some(path) = proposed.path {
                            votes[i].add(&path);
                        }
                    }
                    continue;
                }
                match self.propose(&world, Some(d), first, cap) {
                    Propose::Leaves(leaves) => {
                        let reserve = leaves.iter().find(|line| line.reserves_response());
                        self.report.reserved += u64::from(reserve.is_some());
                        let best = if prefer_reserve {
                            reserve.or_else(|| leaves.first())
                        } else {
                            leaves.first()
                        };
                        if let Some(best) = best {
                            votes[i].add(&best.path);
                        }
                    }
                    Propose::RanOut => {
                        wide_roots[i] = true;
                        if let Some(proposed) =
                            self.propose_wide(&world, first, prefer_reserve, cap)
                        {
                            self.report.reserved += u64::from(proposed.reserved);
                            if let Some(path) = proposed.path {
                                votes[i].add(&path);
                            }
                        }
                    }
                }
            }
        }
        (votes.into_iter().map(Votes::policy).collect(), wide_roots)
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

    /// Advances the line to its next decision, stopping where the turn after the root's would
    /// begin. A line in a subgame plays on through the subgame's turns ([`Self::finished`]).
    fn advance(&mut self, line: &mut Line<L>, limit: u64) -> Option<Step<L>> {
        let limit = self.engine_limit(limit);
        let end = if line.state.in_subgame() {
            AdvanceUntil::WorkLimit
        } else {
            AdvanceUntil::Turn(self.turn.saturating_add(1))
        };
        next_within(
            &mut line.state,
            self.table,
            &mut self.scratch,
            &mut Replay,
            end,
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
        d: Option<&Decision<L>>,
        first: Choice,
        limit: u64,
    ) -> Option<Line<L>> {
        if !self.room(limit, 3 + ITERATION_STEPS) {
            return None;
        }
        self.scratch.counts.copied();
        self.extra += ITERATION_STEPS;
        let mut line = Line::new(world.clone());
        match d {
            Some(d) => self.apply(&mut line, d, first),
            None => apply(
                &mut line.state,
                self.table,
                &mut self.scratch,
                &mut Replay,
                first,
            ),
        }
        smart(&mut line.state);
        Some(line)
    }

    /// Pass priority for the opponent and play required opponent choices from
    /// their own view. Declarations use a combat plan based on the declaring
    /// player's observation, even when a proposed world has extra information.
    fn passes(&self, line: &Line<L>, d: &Decision<L>) -> bool {
        d.kind == DecisionKind::Priority
            && (d.decider != self.viewer
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
            let obs = Observation::new(&line.state, d.decider);
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
        let obs = observe(&line.state, self.table, &mut self.scratch, d.decider);
        Some(self.policy.choose(self.table, obs, d))
    }

    fn propose(
        &mut self,
        world: &State<L>,
        d: Option<&Decision<L>>,
        first: Choice,
        limit: u64,
    ) -> Propose<L> {
        let initial = self.spent();
        let allowance = limit - initial;
        let mut leaves = Beam::default();
        // Keep one pass line and one greedy line ahead of beam pruning.
        for (pass, divisor) in [(true, 4), (false, 2)] {
            let reserved = initial + allowance / divisor;
            if let Some(mut line) = self.start(world, d, first, reserved) {
                if can_widen::<L>() && ran_out(&line.state) {
                    return Propose::RanOut;
                }
                let finished = self.finish(&mut line, None, pass, reserved, None);
                if can_widen::<L>() && ran_out(&line.state) {
                    return Propose::RanOut;
                }
                if finished {
                    self.complete(line, &mut leaves, reserved);
                }
            }
        }
        let Some(line) = self.start(world, d, first, limit) else {
            return Propose::Leaves(leaves.lines.into_iter().map(|(_, line)| line).collect());
        };
        if can_widen::<L>() && ran_out(&line.state) {
            return Propose::RanOut;
        }
        let mut beam = Beam::default();
        beam.push(line, &mut self.report);
        while !beam.lines.is_empty() && self.room(limit, 3) {
            let mut next = Beam::default();
            for (_, mut line) in beam.lines {
                let step = self.advance(&mut line, limit);
                if can_widen::<L>() && ran_out(&line.state) {
                    return Propose::RanOut;
                }
                match step {
                    Some(Step::Decide(d)) => {
                        if !self.expand(line, &d, &mut next, limit) {
                            return Propose::RanOut;
                        }
                    }
                    Some(Step::Over(_)) => self.complete(line, &mut leaves, limit),
                    None if self.finished(&line) => self.complete(line, &mut leaves, limit),
                    None => break,
                }
            }
            beam = next;
        }
        Propose::Leaves(leaves.lines.into_iter().map(|(_, line)| line).collect())
    }

    /// Expands `line` into `next`. Returns `false` if a step ran out of `Small`'s room.
    fn expand(
        &mut self,
        mut line: Line<L>,
        d: &Decision<L>,
        next: &mut Beam<L>,
        limit: u64,
    ) -> bool {
        if !self.room(limit, 4) {
            return true;
        }
        if d.decider != self.viewer || combat::handles(d.kind) || self.passes(&line, d) {
            if let Some(c) = self.forced(&mut line, d, limit)
                && self.room(limit, 2)
            {
                self.apply(&mut line, d, c);
                if can_widen::<L>() && ran_out(&line.state) {
                    return false;
                }
                next.push(line, &mut self.report);
            }
            return true;
        }
        let shown = observe(&line.state, self.table, &mut self.scratch, d.decider);
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
            if can_widen::<L>() && ran_out(&child.state) {
                return false;
            }
            next.push(child, &mut self.report);
        }
        true
    }

    fn complete(&mut self, mut line: Line<L>, leaves: &mut Beam<L>, limit: u64) {
        self.report.completed += 1;
        if !self.room(limit, EVAL_STEPS) {
            return;
        }
        self.extra += EVAL_STEPS;
        let p0 = line.state.outcome().map_or_else(
            || evaluate(&line.state, self.policy.values()),
            |outcome| value(outcome, 0, self.bonus),
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

    /// Whether the line has reached the turn after the root's, or the end of the game. A line
    /// is never finished in a subgame, whose position is of another game than the lines it is
    /// compared with: it plays the subgame out and finishes back in the main game.
    fn finished(&self, line: &Line<L>) -> bool {
        line.state.outcome().is_some()
            || (line.state.elapsed_turns() > self.turn && !line.state.in_subgame())
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
        mut trace: Option<&mut Choices>,
    ) -> bool {
        let mut policy = policy;
        let proposing = policy.is_none();
        while let Some(Step::Decide(d)) = self.advance(line, limit) {
            if !self.room(limit, 4) {
                return false;
            }
            let ours =
                d.decider == self.viewer && !combat::handles(d.kind) && !self.passes(line, &d);
            let seen = (ours && (proposing || policy.is_some())).then(|| {
                let obs = observe(&line.state, self.table, &mut self.scratch, d.decider);
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
            if let Some(trace) = trace.as_deref_mut() {
                trace.push(choice);
            }
            self.apply(line, &d, choice);
            if can_widen::<L>() && ran_out(&line.state) {
                return false;
            }
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
        wide_roots: &[bool],
        rewards: &mut [Vec<f64>],
        rng: &mut SplitMix64,
    ) {
        let limit = agent.settings.work;
        let race = agent.settings.family.options.race;
        // The default is sorted first and kept by `search`.
        let default = roots[0];
        let mut racing = roots.to_vec();
        let leaf = agent.settings.family.leaf;
        let worlds = if agent.settings.family.method == Method::TurnBudget {
            MAX_SCORING_WORLDS
        } else {
            MIN_SCORING_WORLDS
        };
        for scored in 0..worlds {
            if race
                && scored >= MIN_SCORING_WORLDS
                && (scored - MIN_SCORING_WORLDS).is_multiple_of(RACE_INTERVAL)
            {
                self.report.dropped += race_out(&mut racing, rewards, default);
                // The default alone is left: another world would compare nothing.
                if racing.len() == 1 {
                    break;
                }
            }
            if !self.room(limit, WORLD_STEPS + 1) {
                break;
            }
            let world = self.deal(obs, agent.settings.family.sight, rng);
            let seed = rng.next_u64();
            let mut round = Vec::with_capacity(racing.len());
            for &first in &racing {
                let i = usize::from(first.0);
                let reward = if wide_roots[i] {
                    self.score_wide(&world, first, learned[i].as_ref(), None, leaf, seed, limit)
                } else {
                    self.score_line(&world, d, first, learned[i].as_ref(), leaf, seed, limit)
                };
                let Some(reward) = reward else {
                    return;
                };
                crate::mcts::count_rollout();
                round.push((i, reward));
            }
            for (i, reward) in round {
                rewards[i].push(reward);
            }
            self.report.worlds += 1;
        }
    }

    fn reward(&mut self, line: &mut Line<L>, leaf: Leaf, seed: u64, limit: u64) -> Option<f64> {
        let s = &mut line.state;
        let outcome = if let Some(outcome) = s.outcome() {
            Some(outcome)
        } else if leaf == Leaf::Eval && !s.in_subgame() {
            None
        } else {
            let mut rng = SplitMix64::new(seed);
            s.reseed(rng.next_u64());
            let mut soft = Softmax {
                rng: &mut rng,
                temperature: ROLLOUT_TEMPERATURE,
            };
            let cutoff = match leaf {
                Leaf::Cutoff { turns } => s.elapsed_turns().saturating_add(u16::from(turns)),
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
        let turns = s.elapsed_turns().saturating_sub(self.turn);
        let reward = if let Some(outcome) = outcome {
            value(outcome, turns, self.bonus)
        } else {
            if !self.room(limit, EVAL_STEPS) {
                return None;
            }
            self.extra += EVAL_STEPS;
            discounted(evaluate(s, self.policy.values()), turns, self.bonus)
        };
        Some(if self.viewer == Seat::P0 {
            reward
        } else {
            1.0 - reward
        })
    }
}
