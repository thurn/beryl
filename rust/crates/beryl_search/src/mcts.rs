//! Milestone 1's Monte Carlo tree search (architecture §19.1, decision 5 of the Milestone 1
//! plan): UCT over the engine's factored decisions, with full-game rollouts under the rollout
//! policy.
//!
//! - A node is one decision, its children one per candidate. Nodes store choices only: each
//!   iteration copies the tree's root and replays the path down to the node it expands.
//! - Tree nodes play under the smart stops (§15) and pause mid-resolution like the UI (§7.3);
//!   rollouts answer inline. A rollout plays to the end of the game: a win scores 1 for the
//!   winner, a draw or a stall 0.5 for both.
//! - Budgets are iteration counts, and every random stream is seeded from the agent's seed and
//!   the decision's index among its own, so a seed reproduces a move, and an agent rebuilt in
//!   the middle of a game plays on as the one it replaces ([`Mcts::resumed`]).
//! - Independent trees run root-parallel, each on its own thread with no shared mutable
//!   state; their root visit counts are summed in tree order, so the move does not depend on
//!   which thread finishes first. The omniscient search grows its trees on the true state,
//!   each with its own random streams; the hidden one grows one per determinization of its
//!   Observation (ensemble determinization).
//!
//! Three things cut what a decision spends without changing which move a seed makes:
//!
//! - The trees grow in rounds, and stop once no spending of the iterations left could change
//!   the candidate visited most (the exact stop).
//! - Priority with the deciding player's own spell or ability on top of the stack passes
//!   unsearched: the expert never acts there.
//! - A decision asked straight after the one before, as the steps of casting a spell follow
//!   one another (CR 601.2), keeps the trees and starts from the chosen child's subtree: a
//!   budget counts root visits, not fresh rollouts.
//!
//! Every rollout bumps a process-wide counter ([`rollouts_run`]) for a progress display. The
//! search only ever adds to it, relaxed, so it orders nothing and decides nothing.

use beryl_engine::{Large, Layout, SlotId, Small};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Barrier, Mutex};
use std::thread;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome, Profile,
    Replay, Scratch, Seat, State, Step, Stops, apply, next, observe,
};

use crate::rollout::{RolloutPolicy, rollout};
use crate::{Agent, AgentDecision, Widen};

/// What a search may spend on one decision. Iterations, not time, so a seed reproduces a move
/// on any machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Root visits per decision, shared as evenly as they go among the trees: fresh
    /// rollouts, less what trees kept from the decision before bring.
    pub iterations: u32,
    /// Independent trees: the omniscient search's random streams, or the hidden search's
    /// determinizations.
    pub trees: u32,
    /// Whether each tree searches on a thread of its own (root parallelism), or all on the
    /// caller's thread, as tournaments run them: one thread per game.
    pub parallel: bool,
}

impl Budget {
    /// The UI opponent's budget: about 2 s a decision on this project's 18-core Mac (6
    /// performance and 12 efficiency cores), one tree per core, with root parallelism. A
    /// decision waits for the slowest tree, which runs on an efficiency core. Measured over
    /// whole games against the greedy agent, 3600 iterations took 1.1 s a decision and 6000
    /// took 3.3 s; a decision's cost follows how long its rollouts' games run.
    pub const DEFAULT: Budget = Budget {
        iterations: 4000,
        trees: 18,
        parallel: true,
    };

    /// Tree `i`'s share of the iterations.
    fn share(self, i: u32) -> u32 {
        let trees = self.trees.max(1);
        self.iterations / trees + u32::from(i < self.iterations % trees)
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the search may know of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sight {
    /// The true state, hidden cards and library order included: cheating.
    Omniscient,
    /// Only its Observation: each tree searches a determinization of it.
    Hidden,
    /// Its Observation and every hand, the libraries dealt at random: half-cheating, to
    /// measure what seeing the opponent's hand is worth apart from knowing the draws.
    Hands,
}

/// The UCT exploration constant, for rewards in [0, 1]. Against the greedy agent, 72 games a
/// constant at 100 iterations and 4 trees, the hidden search won 44% at 0.35, 51% at 0.7
/// and 43% at 1.4: the best of the three, though the intervals overlap.
const EXPLORATION: f64 = 0.7;

/// The rounds a tree's share is grown in, the exact stop checked after each. More stop
/// sooner, at a wait for the slowest tree each.
const ROUNDS: u32 = 16;

/// The rollouts every search in the process has run.
static ROLLOUTS: AtomicU64 = AtomicU64::new(0);

/// The rollouts every search in this process has run so far, as a decision's trees grow: a
/// progress display reads the difference across a decision.
pub fn rollouts_run() -> u64 {
    ROLLOUTS.load(Ordering::Relaxed)
}

/// Counts one iteration for [`rollouts_run`].
pub(crate) fn count_rollout() {
    ROLLOUTS.fetch_add(1, Ordering::Relaxed);
}

/// What one searched decision found and spent, for a runner's log and as training data.
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    /// The decision's index among the agent's own.
    pub decision: AgentDecision,
    /// The decision whose fresh search planted the trees this one grew: `decision`, unless it
    /// reused the trees of the decisions before it.
    pub chain: AgentDecision,
    /// The rollouts it ran.
    pub rollouts: u32,
    /// The root visits its trees brought from the decision before.
    pub inherited: u32,
    /// The engine steps it spent: every decision `next` returned or settled, in its trees
    /// and their rollouts, and for the long-think searcher its worlds and evaluations.
    pub work: u64,
    /// The root visits per candidate, summed across the trees, when the search stopped.
    pub visits: Vec<u32>,
    /// The rewards per candidate for the deciding player, summed likewise. Divided by the
    /// visits they pool the trees' worlds visit-weighted, which misleads: see `means`.
    pub rewards: Vec<f64>,
    /// Each candidate's mean reward for the deciding player, averaged over the trees that
    /// visited it with each tree weighing the same; none for one no tree visited. Pooling
    /// instead (`rewards / visits`) weighs a candidate's mean toward the trees whose world
    /// favours it, since each tree spends its visits on its own best candidate: with worlds
    /// that disagree, the candidate visited most can have the lower pooled mean (Simpson's
    /// paradox; `docs/incidents/2026-11-08-searcher-mulligan-means.md`).
    pub means: Vec<Option<f64>>,
    /// For an anchored long-think search, its check of its choice against greedy's answer.
    pub anchor: Option<crate::long::AnchorCheck>,
}

/// The searching agent.
#[derive(Debug)]
pub struct Mcts<L: Layout = Small> {
    table: GameTable,
    policy: RolloutPolicy,
    sight: Sight,
    budget: Budget,
    seed: u64,
    /// The next decision's index among the agent's own.
    decision: AgentDecision,
    /// The last decision's trees, while its chosen child may root the next one's.
    kept: Option<Kept<L>>,
    last: Option<Search>,
}

/// The trees of a decision just answered, and what reusing them for the next one requires.
#[derive(Debug)]
struct Kept<L: Layout> {
    trees: Vec<Tree<L>>,
    chain: AgentDecision,
    choice: Choice,
    /// Each slot's incarnation, and whether the deciding player knew its card, then.
    seen: Vec<(u16, bool)>,
    /// The card the choice names, which it may move: the spell cast, the land played.
    moves: Option<L::Slot>,
}

impl<L: Layout> Mcts<L> {
    pub fn new(t: &GameTable, sight: Sight, budget: Budget, seed: u64) -> Self {
        Self::resumed(t, sight, budget, seed, AgentDecision(0))
    }

    /// The agent built from `seed` as it stands before its own decision `decision`, for a
    /// game resumed midway: it plays on as the uninterrupted agent would, if it is first
    /// asked the decision [`Agent::resume_point`] named.
    pub fn resumed(
        t: &GameTable,
        sight: Sight,
        budget: Budget,
        seed: u64,
        decision: AgentDecision,
    ) -> Self {
        Self {
            table: t.clone(),
            policy: RolloutPolicy::new(t),
            sight,
            budget,
            seed,
            decision,
            kept: None,
            last: None,
        }
    }

    /// The root visit counts of every tree, summed, one per candidate of `d`: what a fresh
    /// search of the next decision on its full budget counts, with no stop. Changes nothing.
    pub fn visits(&self, obs: Observation<'_, L>, d: &Decision<L>) -> Vec<u32> {
        let mut trees = self.plant(obs, d, self.decision);
        self.grow(&mut trees, false);
        totals(&trees).0
    }

    /// One tree per [`Budget::trees`], seeded from the agent's seed and the decision's index.
    fn plant(
        &self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        decision: AgentDecision,
    ) -> Vec<Tree<L>> {
        // The `decision`th output of the agent's stream, computed directly.
        let mut seeds = SplitMix64::new(
            SplitMix64::new(
                self.seed
                    .wrapping_add(u64::from(decision.0).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
            )
            .next_u64(),
        );
        (0..self.budget.trees.max(1))
            .map(|_| Tree::new(&self.table, self.sight, obs, d, seeds.next_u64()))
            .collect()
    }

    /// Grows each tree to its share of root visits, in rounds; with `stop`, ends after the
    /// round that settles the candidate visited most. Returns the rollouts run.
    fn grow(&self, trees: &mut [Tree<L>], stop: bool) -> u32 {
        let mut plans: Vec<Plan> = (0..)
            .zip(trees.iter())
            .map(|(i, tree)| {
                let share = self.budget.share(i);
                Plan {
                    left: share.saturating_sub(tree.visits()),
                    slice: if stop {
                        share.div_ceil(ROUNDS).max(1)
                    } else {
                        u32::MAX
                    },
                }
            })
            .collect();
        let before = plans.iter().map(|p| p.left).sum::<u32>();
        let (t, policy) = (&self.table, &self.policy);
        if self.budget.parallel && trees.len() > 1 {
            grow_parallel(t, policy, trees, &mut plans, stop);
        } else {
            while !finished(
                trees
                    .iter()
                    .map(Tree::counts)
                    .zip(plans.iter().map(|p| p.left)),
                stop,
            ) {
                for (tree, plan) in trees.iter_mut().zip(&mut plans) {
                    tree.grow(t, policy, plan.take());
                }
            }
        }
        before - plans.iter().map(|p| p.left).sum::<u32>()
    }

    /// The trees of the last decision, moved down to its chosen child, if `d` may reuse them:
    /// asked as this agent's next decision (any other of its own takes the kept trees), no
    /// other card changed zones and the player learned nothing since, and every tree reached
    /// `d` straight from that child, with no other player's decision between, showing what
    /// the game shows. Each tree keeps its determinization, which nothing since has
    /// contradicted. Whether the other player was offered a decision in the game is not
    /// asked: with their passes silent when they have nothing to do, it would betray their
    /// hand.
    fn descend(
        &self,
        kept: Kept<L>,
        obs: Observation<'_, L>,
        d: &Decision<L>,
    ) -> Option<Vec<Tree<L>>> {
        // CR 400.7: "An object that moves from one zone to another becomes a new object
        // with no memory of or relation to its previous existence", so a new incarnation is
        // a zone change.
        for (slot, &(incarnation, knew)) in L::Slot::all().zip(&kept.seen) {
            let o = obs.object(slot);
            if obs.knows(slot) != knew
                || (o.incarnation() != incarnation && kept.moves != Some(slot))
            {
                return None;
            }
        }
        let mut trees = kept.trees;
        for tree in &mut trees {
            if !tree.descend(&self.table, kept.choice, obs, d) {
                return None;
            }
        }
        Some(trees)
    }

    /// Whether `d` is priority with the player's own spell or ability on top of the stack,
    /// which they pass unsearched. Over 400 expert mirror games that was 27 of 159
    /// decisions per seat a game, and the expert never acted there.
    fn passes(obs: Observation<'_, L>, d: &Decision<L>) -> bool {
        d.kind == DecisionKind::Priority
            && obs.stack().last().is_some_and(|e| e.controller == d.player)
    }
}

/// The same search in [`Large`], at the same place among its own decisions. The trees it kept
/// hold `Small` states, so they are dropped: its next search plants fresh ones.
impl Widen for Mcts<Small> {
    type Large = Mcts<Large>;

    fn widen(&self) -> Mcts<Large> {
        Mcts {
            table: self.table.clone(),
            policy: self.policy.clone(),
            sight: self.sight,
            budget: self.budget,
            seed: self.seed,
            decision: self.decision,
            kept: None,
            last: None,
        }
    }
}

impl<L: Layout> Agent<L> for Mcts<L> {
    /// The candidate visited most across the trees, the first of equals. A decision with one
    /// candidate is never searched, nor is passing with one's own object on top of the stack.
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Choice {
        let decision = self.decision;
        self.decision = decision.next();
        self.last = None;
        let kept = self.kept.take();
        if d.len() < 2 {
            return Choice(0);
        }
        if Self::passes(obs, d) {
            debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
            return Choice(0);
        }
        let chain = kept.as_ref().map(|k| k.chain);
        let (mut trees, chain) = match kept.and_then(|k| self.descend(k, obs, d)) {
            Some(trees) => (trees, chain.expect("kept trees")),
            None => (self.plant(obs, d, decision), decision),
        };
        let inherited = trees.iter().map(Tree::visits).sum();
        let before: u64 = trees.iter().map(Tree::steps).sum();
        let rollouts = self.grow(&mut trees, true);
        let work = trees.iter().map(Tree::steps).sum::<u64>() - before;
        let (visits, rewards) = totals(&trees);
        let choice = Choice(leader(&visits) as u16);
        let means = tree_means(
            visits.len(),
            trees
                .iter()
                .map(|tree| tree.children().iter().map(|c| Some((c.visits, c.reward)))),
        );
        self.last = Some(Search {
            decision,
            chain,
            rollouts,
            inherited,
            work,
            visits,
            rewards,
            means,
            anchor: None,
        });
        if trees.iter().all(|tree| tree.expanded(choice)) {
            self.kept = Some(Kept {
                trees,
                chain,
                choice,
                seen: L::Slot::all()
                    .map(|slot| (obs.object(slot).incarnation(), obs.knows(slot)))
                    .collect(),
                moves: names(d, choice),
            });
        }
        choice
    }

    fn last_search(&self) -> Option<&Search> {
        self.last.as_ref()
    }

    /// The next decision, unless this agent keeps trees the next may reuse, then the
    /// decision whose fresh search planted them, re-searched from there.
    fn resume_point(&self) -> Option<AgentDecision> {
        Some(self.kept.as_ref().map_or(self.decision, |k| k.chain))
    }

    fn next_decision(&self) -> Option<AgentDecision> {
        Some(self.decision)
    }

    fn rewind(&mut self, to: AgentDecision) {
        self.decision = to;
        self.kept = None;
        self.last = None;
    }
}

/// What a tree has left to grow, and how much of it a round grows.
#[derive(Clone, Copy, Debug)]
struct Plan {
    left: u32,
    slice: u32,
}

impl Plan {
    /// The iterations of the next round.
    fn take(&mut self) -> u32 {
        let n = self.left.min(self.slice);
        self.left -= n;
        n
    }
}

/// [`Mcts::grow`] with each tree on its own thread. Between rounds every thread reads the
/// same board of root visit counts, so all reach the same verdict on whether to go on.
fn grow_parallel<L: Layout>(
    t: &GameTable,
    policy: &RolloutPolicy,
    trees: &mut [Tree<L>],
    plans: &mut [Plan],
    stop: bool,
) {
    let board: Vec<Mutex<(Vec<u32>, u32)>> = trees
        .iter()
        .zip(plans.iter())
        .map(|(tree, plan)| Mutex::new((tree.counts(), plan.left)))
        .collect();
    let barrier = Barrier::new(trees.len());
    let failed = Mutex::new(None);
    thread::scope(|scope| {
        for ((tree, plan), slot) in trees.iter_mut().zip(plans.iter_mut()).zip(&board) {
            let (board, barrier, failed) = (&board, &barrier, &failed);
            scope.spawn(move || {
                loop {
                    *slot.lock().expect("a board slot") = (tree.counts(), plan.left);
                    barrier.wait();
                    let done = failed.lock().expect("the failure").is_some()
                        || finished(
                            board
                                .iter()
                                .map(|s| s.lock().expect("a board slot").clone()),
                            stop,
                        );
                    // Every thread reads the board before any writes it again.
                    barrier.wait();
                    if done {
                        break;
                    }
                    let n = plan.take();
                    // A panicking tree still meets every barrier, so the others end with it
                    // rather than wait for it forever.
                    if let Err(e) = catch_unwind(AssertUnwindSafe(|| tree.grow(t, policy, n))) {
                        failed.lock().expect("the failure").get_or_insert(e);
                    }
                }
            });
        }
    });
    if let Some(e) = failed.into_inner().expect("the failure") {
        resume_unwind(e);
    }
}

/// Whether trees with these root visit counts and iterations left are done: when none are
/// left, or with `stop` once the candidate visited most is settled, since no candidate could
/// overtake it even if every iteration left went its way. The first of equals wins, so one
/// before the leader must stay strictly behind.
fn finished(trees: impl Iterator<Item = (Vec<u32>, u32)>, stop: bool) -> bool {
    let mut visits: Vec<u32> = Vec::new();
    let mut left = 0;
    for (counts, l) in trees {
        visits.resize(counts.len(), 0);
        for (sum, n) in visits.iter_mut().zip(counts) {
            *sum += n;
        }
        left += l;
    }
    if left == 0 || !stop {
        return left == 0;
    }
    let best = leader(&visits);
    visits
        .iter()
        .enumerate()
        .all(|(i, &n)| i == best || visits[best] >= n + left + u32::from(i < best))
}

/// The candidate visited most, the first of equals.
fn leader(visits: &[u32]) -> usize {
    let mut best = 0;
    for (i, &n) in visits.iter().enumerate() {
        if n > visits[best] {
            best = i;
        }
    }
    best
}

/// The root visits and rewards per candidate, summed across `trees` in order.
fn totals<L: Layout>(trees: &[Tree<L>]) -> (Vec<u32>, Vec<f64>) {
    let n = trees.first().map_or(0, |t| t.children().len());
    let mut visits = vec![0; n];
    let mut rewards = vec![0.0; n];
    for tree in trees {
        for ((v, r), child) in visits.iter_mut().zip(&mut rewards).zip(tree.children()) {
            *v += child.visits;
            *r += child.reward;
        }
    }
    (visits, rewards)
}

/// Each of `n` candidates' mean reward averaged over the trees, each tree weighing the same:
/// every tree gives each candidate its (visits, summed reward), or none where it has no such
/// child. A candidate a tree never visited is left out of that tree's share, and one no tree
/// visited has no mean.
pub(crate) fn tree_means<I>(n: usize, trees: impl IntoIterator<Item = I>) -> Vec<Option<f64>>
where
    I: IntoIterator<Item = Option<(u32, f64)>>,
{
    let mut sums = vec![(0.0, 0u32); n];
    for tree in trees {
        for ((sum, count), child) in sums.iter_mut().zip(tree) {
            if let Some((visits, reward)) = child.filter(|&(v, _)| v > 0) {
                *sum += reward / f64::from(visits);
                *count += 1;
            }
        }
    }
    sums.into_iter()
        .map(|(sum, count)| (count > 0).then(|| sum / f64::from(count)))
        .collect()
}

/// The card a choice names, which answering it may move: the card cast, the land played,
/// the source activated (a cost may sacrifice it), the object picked.
fn names<L: Layout>(d: &Decision<L>, c: Choice) -> Option<L::Slot> {
    match d.candidates {
        Candidates::Objects(_) => d.candidates.object(c),
        Candidates::Options(_) => match d.candidates.option(c)? {
            Opt::Cast { slot, .. }
            | Opt::PlayLand(slot)
            | Opt::Activate { slot, .. }
            | Opt::Object(slot) => Some(slot),
            _ => None,
        },
    }
}

/// One decision in a tree. Its children, one per candidate, are contiguous in the arena.
#[derive(Clone, Copy, Debug)]
struct Node {
    visits: u32,
    /// The sum of the rewards of the games through this node, for `mover`.
    reward: f64,
    /// The player whose choice leads here.
    mover: Seat,
    /// The first child's index, and how many there are; none until the node is expanded.
    first: u32,
    children: u8,
}

impl Node {
    fn new(mover: Seat) -> Self {
        Self {
            visits: 0,
            reward: 0.0,
            mover,
            first: 0,
            children: 0,
        }
    }
}

/// One tree and what growing it needs.
#[derive(Debug)]
struct Tree<L: Layout> {
    nodes: Vec<Node>,
    /// The node of the decision being searched.
    root: u32,
    /// The position at the root: the true state or a determinization of it.
    state: State<L>,
    rng: SplitMix64,
    x: Scratch<L>,
}

impl<L: Layout> Tree<L> {
    /// A tree of one expanded root, for the position `obs` shows at decision `d`.
    fn new(
        t: &GameTable,
        sight: Sight,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        seed: u64,
    ) -> Self {
        let mut rng = SplitMix64::new(seed);
        let state = match sight {
            Sight::Omniscient => {
                let mut s = obs.unmasked_root().clone();
                s.reseed(rng.next_u64());
                s
            }
            Sight::Hidden => obs.determinize(t, &mut rng),
            Sight::Hands => obs.determinize_libraries(t, &mut rng),
        };
        let mut tree = Tree {
            nodes: vec![Node::new(d.player)],
            root: 0,
            state,
            rng,
            x: Scratch::unchecked(),
        };
        tree.expand(0, d);
        tree
    }

    fn visits(&self) -> u32 {
        self.nodes[self.root as usize].visits
    }

    /// The engine steps the tree's scratch has counted over its life.
    fn steps(&self) -> u64 {
        self.x.counts.decisions + self.x.counts.silent
    }

    fn children(&self) -> &[Node] {
        let root = self.nodes[self.root as usize];
        &self.nodes[root.first as usize..][..usize::from(root.children)]
    }

    fn counts(&self) -> Vec<u32> {
        self.children().iter().map(|n| n.visits).collect()
    }

    /// Whether the root's child for `c` has children of its own, so it can root a search.
    fn expanded(&self, c: Choice) -> bool {
        self.children()[usize::from(c.0)].children > 0
    }

    fn grow(&mut self, t: &GameTable, policy: &RolloutPolicy, iterations: u32) {
        for _ in 0..iterations {
            self.iterate(t, policy);
            ROLLOUTS.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Moves the root down to its child for `c`, if the game there reaches `d` with no other
    /// decision between and shows everything `obs` shows: had another player acted in the
    /// game, a tapped permanent, a counter or an entry on the stack would differ.
    fn descend(
        &mut self,
        t: &GameTable,
        c: Choice,
        obs: Observation<'_, L>,
        d: &Decision<L>,
    ) -> bool {
        let child = self.nodes[self.root as usize].first + u32::from(c.0);
        let mut s = self.state.clone();
        self.x.counts.copied();
        apply(&mut s, t, &mut self.x, &mut Replay, c);
        smart(&mut s);
        if next(&mut s, t, &mut self.x, &mut Replay) != Step::Decide(d.clone()) {
            return false;
        }
        if !shows_the_same(observe(&s, t, &mut self.x, d.player), obs) {
            return false;
        }
        self.state = s;
        self.root = child;
        true
    }

    /// One iteration: from a copy of the root, select by UCB1 down to a child not yet played,
    /// play it out, and score the game for every node on the way.
    fn iterate(&mut self, t: &GameTable, policy: &RolloutPolicy) {
        let mut s = self.state.clone();
        self.x.counts.copied();
        let mut path: [u32; 256] = [0; 256];
        let mut depth = 0;
        let mut node = self.root;
        let outcome = loop {
            let child = self.select(node);
            path[depth] = child;
            depth += 1;
            let choice = Choice((child - self.nodes[node as usize].first) as u16);
            apply(&mut s, t, &mut self.x, &mut Replay, choice);
            if node == self.root {
                smart(&mut s);
            }
            if self.nodes[child as usize].visits == 0 || depth == path.len() {
                s.reseed(self.rng.next_u64());
                break rollout(&mut s, t, &mut self.x, policy);
            }
            match next(&mut s, t, &mut self.x, &mut Replay) {
                Step::Over(outcome) => break outcome,
                Step::Decide(below) => {
                    if self.nodes[child as usize].children == 0 {
                        self.expand(child, &below);
                    }
                    node = child;
                }
            }
        };
        self.nodes[self.root as usize].visits += 1;
        for &n in &path[..depth] {
            let n = &mut self.nodes[n as usize];
            n.visits += 1;
            n.reward += score(outcome, n.mover);
        }
    }

    /// Gives `node` one child per candidate of its decision `d`.
    fn expand(&mut self, node: u32, d: &Decision<L>) {
        let first = self.nodes.len() as u32;
        self.nodes.extend((0..d.len()).map(|_| Node::new(d.player)));
        let n = &mut self.nodes[node as usize];
        n.first = first;
        n.children = d.len() as u8;
    }

    /// The child of `node` to play next: one not yet played, chosen at random, else the one
    /// with the highest UCB1 bound for the player choosing, the first of equals.
    fn select(&mut self, node: u32) -> u32 {
        let n = self.nodes[node as usize];
        let children = n.first..n.first + u32::from(n.children);
        let fresh = children
            .clone()
            .filter(|&c| self.nodes[c as usize].visits == 0)
            .count() as u64;
        if fresh > 0 {
            let pick = self.rng.below(fresh) as usize;
            return children
                .filter(|&c| self.nodes[c as usize].visits == 0)
                .nth(pick)
                .expect("a fresh child");
        }
        let log = f64::from(n.visits).ln();
        let mut best = (n.first, f64::NEG_INFINITY);
        for c in children {
            let child = &self.nodes[c as usize];
            let visits = f64::from(child.visits);
            let bound = child.reward / visits + EXPLORATION * (log / visits).sqrt();
            if bound > best.1 {
                best = (c, bound);
            }
        }
        best.0
    }
}

/// Whether two views of a game show the same: the turn, the players, the stack, the
/// waiting triggers and permissions, and every slot's object as far as the viewer knows it.
pub(crate) fn shows_the_same<L: Layout>(a: Observation<'_, L>, b: Observation<'_, L>) -> bool {
    a.turn() == b.turn()
        && Seat::BOTH.iter().all(|&p| a.player(p) == b.player(p))
        && a.stack() == b.stack()
        && a.triggers() == b.triggers()
        && a.permissions() == b.permissions()
        && L::Slot::all()
            .all(|slot| a.knows(slot) == b.knows(slot) && a.object(slot) == b.object(slot))
}

/// The profile below a tree's root. The root decision was offered under the game's profile;
/// below it the tree plays under the smart stops, with no player's passes asked, no mana
/// floated ahead of a payment and no activation offered that would do nothing. The payment profile stays, so a payment in progress is never
/// asked under another. The budgets go back to search's, which a player game lifts
/// ([`State::search_limits`]).
pub(crate) fn smart<L: Layout>(s: &mut State<L>) {
    s.set_profile(Profile {
        stops: Stops::Smart,
        limits: s.search_limits(),
        breakpoint: None,
        full_control: None,
        asks_targets: None,
        taps_mana: None,
        hides_pointless: [true; 2],
        quiet_pointless: None,
        ..s.profile()
    });
}

/// A finished game's reward for `player`: 1 for a win, 0 for a loss, 0.5 for a draw or a
/// stall.
fn score(outcome: Outcome, player: Seat) -> f64 {
    match outcome {
        Outcome::Win(winner) if winner == player => 1.0,
        Outcome::Win(_) => 0.0,
        Outcome::Draw | Outcome::Stalled(_) => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use beryl_cards::CARDS;
    use beryl_cards::decks::WU_CONTROL;
    use beryl_engine::{GameTable, Limits, Profile, Small, State};

    use super::{finished, leader, smart, tree_means};

    #[test]
    fn a_tree_plays_under_search_budgets_below_a_player_game() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let player = Profile {
            limits: Limits::PLAYER,
            ..Profile::SMART
        };
        let mut s: State<Small> = State::new(&t, 0, player);
        smart(&mut s);
        assert_eq!(s.profile().limits, Limits::DEFAULT);
    }

    /// Trees whose worlds disagree, as the hidden search's mulligan in the incident did: each
    /// spends its visits on its own best candidate. Pooled, the rewards rank the candidates
    /// against their visits; averaged tree by tree, they rank them as the visits do.
    #[test]
    fn tree_means_weigh_each_tree_the_same() {
        let trees = [
            [(200, 160.0), (10, 1.0)],
            [(10, 0.0), (200, 120.0)],
            [(10, 0.0), (200, 100.0)],
        ];
        let pooled: Vec<f64> = (0..2)
            .map(|i| {
                let (v, r) = trees
                    .iter()
                    .fold((0, 0.0), |(v, r), t| (v + t[i].0, r + t[i].1));
                r / f64::from(v)
            })
            .collect();
        assert!(pooled[0] > pooled[1], "{pooled:?}");
        let means = tree_means(2, trees.iter().map(|t| t.iter().map(|&c| Some(c))));
        let [Some(keep), Some(mulligan)] = means[..] else {
            panic!("{means:?}")
        };
        assert!((keep - 0.8 / 3.0).abs() < 1e-9 && (mulligan - 1.2 / 3.0).abs() < 1e-9);
        assert_eq!(
            tree_means(2, [[Some((0, 0.0)), None], [Some((4, 2.0)), None]]),
            vec![Some(0.5), None]
        );
    }

    /// Every way to hand `left` more root visits to the candidates of `visits`.
    fn spendings(visits: &[u32], left: u32, out: &mut Vec<Vec<u32>>) {
        match visits.split_first() {
            None => out.push(Vec::new()),
            Some((&first, [])) => out.push(vec![first + left]),
            Some((&first, rest)) => {
                for n in 0..=left {
                    let mut tails = Vec::new();
                    spendings(rest, left - n, &mut tails);
                    out.extend(tails.into_iter().map(|mut tail| {
                        tail.insert(0, first + n);
                        tail
                    }));
                }
            }
        }
    }

    /// The exact stop ends a search exactly when every spending of the iterations left keeps
    /// the candidate visited most, the first of equals, split across trees or not.
    #[test]
    fn the_exact_stop_ends_once_the_leader_is_settled() {
        let mut stopped = 0;
        for candidates in 2..=3 {
            for code in 0..6u32.pow(candidates) {
                let visits: Vec<u32> = (0..candidates).map(|i| code / 6u32.pow(i) % 6).collect();
                for left in 0..6 {
                    let mut all = Vec::new();
                    spendings(&visits, left, &mut all);
                    let settled = all.iter().all(|v| leader(v) == leader(&visits));
                    let one = finished([(visits.clone(), left)].into_iter(), true);
                    let halves: Vec<u32> = visits.iter().map(|n| n / 2).collect();
                    let rest: Vec<u32> = visits.iter().zip(&halves).map(|(n, h)| n - h).collect();
                    let two = finished(
                        [(halves, left / 2), (rest, left - left / 2)].into_iter(),
                        true,
                    );
                    assert_eq!((one, two), (settled, settled), "{visits:?}, {left} left");
                    assert_eq!(
                        finished([(visits.clone(), left)].into_iter(), false),
                        left == 0
                    );
                    stopped += u32::from(settled && left > 0);
                }
            }
        }
        assert!(stopped > 0);
    }
}
