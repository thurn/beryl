//! Play two root choices from the same world with the same rollout seed.

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, Decision, GameTable, Layout, Observation, Outcome, Scratch, Seat, State,
};

use super::{
    AgentDecision, AnchorCheck, FREE_ROUNDS, ITERATION_STEPS, LongSearch, ROLLOUT_TEMPERATURE,
    ROUNDS, RolloutPolicy, Sight, Tree, WORLD_STEPS, check, deal, value,
};
use crate::rollout::{Softmax, soft_rollout_after_within};

/// How many standard errors a paired lead must clear before search can override the policy.
pub const PAIRED_ERRORS: f64 = 3.0;

/// Limits for distributing the comparison over workers. Each share needs room for multiple pairs.
const MAX_SHARES: u64 = 32;
const STEPS_PER_SHARE: u64 = 16_000;
/// Dealing one world and making two independently playable copies.
const PAIR_STEPS: u64 = WORLD_STEPS + 2 * ITERATION_STEPS;

impl<L: Layout> LongSearch<L> {
    pub(super) fn paired_rollouts<'a>(
        &'a self,
        obs: Observation<'a, L>,
        decision: AgentDecision,
    ) -> PairedRollouts<'a, L> {
        let seed =
            self.seed.wrapping_add(u64::from(decision.0) * 0x9E37_79B9) ^ 0xA396_749C_8712_CB7D;
        PairedRollouts::new(
            &self.table,
            &self.policy,
            obs,
            self.settings.family.sight,
            seed,
        )
    }

    pub(super) fn check_challenger(
        &self,
        trees: &[Tree<L>],
        obs: Observation<'_, L>,
        decision: AgentDecision,
        choices: [Choice; 2],
        tree_work: u64,
    ) -> (AnchorCheck, u64) {
        if !self.settings.family.options.paired {
            let [search, default] = choices;
            return (check(trees, search, default), 0);
        }
        // A guarded challenger that emerged in the last four tree rounds can use only
        // the work those rounds left. Without enough pairs, the default stands.
        let reserve = self.settings.work / (ROUNDS / (ROUNDS - FREE_ROUNDS));
        let budget = reserve.min(self.settings.work.saturating_sub(tree_work));
        self.paired_rollouts(obs, decision)
            .check(choices, budget, self.settings.threads)
    }
}

/// A source of independently seeded worlds for comparing choices at one decision.
pub struct PairedRollouts<'a, L: Layout> {
    table: &'a GameTable,
    policy: &'a RolloutPolicy,
    observation: Observation<'a, L>,
    sight: Sight,
    seed: u64,
}

impl<'a, L: Layout> PairedRollouts<'a, L> {
    pub fn new(
        table: &'a GameTable,
        policy: &'a RolloutPolicy,
        observation: Observation<'a, L>,
        sight: Sight,
        seed: u64,
    ) -> Self {
        Self {
            table,
            policy,
            observation,
            sight,
            seed,
        }
    }

    /// Deal the world with this index. Both moves start with its state and random stream.
    pub fn sample(&self, index: u64) -> RolloutPair<'a, L> {
        let mut rng = SplitMix64::new(self.seed.wrapping_add(index));
        let world = deal(self.table, self.sight, self.observation, &mut rng);
        RolloutPair {
            table: self.table,
            policy: self.policy,
            world,
            rng,
            viewer: self.observation.viewer(),
        }
    }

    /// Compare a challenger with the default within a budget of engine steps. Only complete
    /// pairs contribute scores. Work includes both the finished pairs and any partial pair.
    pub fn check(
        &self,
        [search, greedy]: [Choice; 2],
        budget: u64,
        threads: u32,
    ) -> (AnchorCheck, u64) {
        // Each share owns its own budget and pair indices. Partition and sum in the same order
        // on any number of threads so both the sample and the floating-point result are stable.
        let count = (budget / STEPS_PER_SHARE).clamp(1, MAX_SHARES);
        let mut shares: Vec<_> = (0..count)
            .map(|index| Share {
                next: index,
                limit: budget / count + u64::from(index < budget % count),
                spent: 0,
                scores: PairedScores::default(),
            })
            .collect();
        let grow = |share: &mut Share, _: usize, held: &dyn Fn() -> bool| {
            let mut scratch = Scratch::unchecked();
            while share.limit - share.spent >= PAIR_STEPS {
                if held() {
                    return false;
                }
                let pair = self.sample(share.next);
                share.next += count;
                share.spent += PAIR_STEPS;
                let before = scratch.counts.decision_work();
                let limit = before + (share.limit - share.spent);
                let result = [search, greedy]
                    .map(|choice| pair.play_within(choice, &mut scratch, limit, |_, _, _| {}));
                share.spent += scratch.counts.decision_work() - before;
                let [(a, Some(outcome_a)), (b, Some(outcome_b))] = result else {
                    break;
                };
                crate::mcts::count_rollout();
                crate::mcts::count_rollout();
                share
                    .scores
                    .add([pair.score(&a, outcome_a), pair.score(&b, outcome_b)]);
            }
            true
        };
        let threads = (threads as usize).clamp(1, shares.len());
        if threads == 1 {
            for (i, share) in shares.iter_mut().enumerate() {
                grow(share, i, &|| false);
            }
        } else {
            crate::pace::share(&mut shares, threads, grow);
        }
        let mut scores = PairedScores::default();
        let mut work = 0;
        for share in shares {
            scores.merge(&share.scores);
            work += share.spent;
        }
        (
            AnchorCheck {
                greedy,
                search,
                lead: scores.lead(),
                error: scores.error().unwrap_or(0.0),
                trees: 0,
                pairs: Some(scores.count),
            },
            work,
        )
    }
}

struct Share {
    next: u64,
    limit: u64,
    spent: u64,
    scores: PairedScores,
}

/// A dealt world that can replay either choice and its continuation.
pub struct RolloutPair<'a, L: Layout> {
    table: &'a GameTable,
    policy: &'a RolloutPolicy,
    world: State<L>,
    rng: SplitMix64,
    viewer: Seat,
}

impl<L: Layout> RolloutPair<'_, L> {
    pub fn world(&self) -> &State<L> {
        &self.world
    }

    /// Apply the forced choice and play to the end, recording the rollout policy's answers.
    pub fn play(
        &self,
        choice: Choice,
        scratch: &mut Scratch<L>,
        record: impl FnMut(Observation<'_, L>, &Decision<L>, Choice),
    ) -> (State<L>, Outcome) {
        let (state, outcome) = self.play_within(choice, scratch, u64::MAX, record);
        (state, outcome.expect("a game ends before turn 65535"))
    }

    fn play_within(
        &self,
        choice: Choice,
        scratch: &mut Scratch<L>,
        limit: u64,
        record: impl FnMut(Observation<'_, L>, &Decision<L>, Choice),
    ) -> (State<L>, Option<Outcome>) {
        let mut state = self.world.clone();
        let mut rng = self.rng;
        let mut soft = Softmax {
            rng: &mut rng,
            temperature: ROLLOUT_TEMPERATURE,
        };
        let outcome = soft_rollout_after_within(
            &mut state,
            self.table,
            scratch,
            self.policy,
            &mut soft,
            choice,
            record,
            limit,
        );
        (state, outcome)
    }

    /// Turns since this pair's root.
    pub fn turns(&self, state: &State<L>) -> u16 {
        state
            .elapsed_turns()
            .saturating_sub(self.world.elapsed_turns())
    }

    /// The search's time-discounted reward for the player at the root.
    pub fn score(&self, state: &State<L>, outcome: Outcome) -> f64 {
        let p0 = value(outcome, self.turns(state));
        if self.viewer == Seat::P0 {
            p0
        } else {
            1.0 - p0
        }
    }
}

/// Means and standard error from matched observations of two choices.
#[derive(Default)]
pub struct PairedScores {
    count: u64,
    totals: [f64; 2],
    differences: f64,
    squared_differences: f64,
}

impl PairedScores {
    pub fn add(&mut self, [a, b]: [f64; 2]) {
        self.count += 1;
        self.totals[0] += a;
        self.totals[1] += b;
        self.differences += a - b;
        self.squared_differences += (a - b).powi(2);
    }

    fn merge(&mut self, other: &Self) {
        self.count += other.count;
        self.totals[0] += other.totals[0];
        self.totals[1] += other.totals[1];
        self.differences += other.differences;
        self.squared_differences += other.squared_differences;
    }

    pub fn means(&self) -> [f64; 2] {
        self.totals.map(|sum| sum / self.count.max(1) as f64)
    }

    pub fn lead(&self) -> f64 {
        self.differences / self.count.max(1) as f64
    }

    pub fn error(&self) -> Option<f64> {
        let n = self.count as f64;
        (self.count >= 2).then(|| {
            let variance = (self.squared_differences / n - self.lead().powi(2)).max(0.0);
            (variance / (n - 1.0)).sqrt()
        })
    }
}
