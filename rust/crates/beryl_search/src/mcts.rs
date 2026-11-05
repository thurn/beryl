//! Milestone 1's Monte Carlo tree search (architecture §19.1, decision 5 of the Milestone 1
//! plan): UCT over the engine's factored decisions, with full-game rollouts under the rollout
//! policy.
//!
//! - A node is one decision, its children one per candidate. Nodes store choices only: each
//!   iteration copies the tree's root and replays the path down to the node it expands.
//! - Tree nodes play under the smart stops (§15) and pause mid-resolution like the UI (§7.3);
//!   rollouts answer inline. A rollout plays to the end of the game: a win scores 1 for the
//!   winner, a draw or a stall 0.5 for both.
//! - Budgets are iteration counts, and every random stream is seeded from the agent's own, so
//!   a seed reproduces a move.
//! - Independent trees run root-parallel, each on its own thread with no shared mutable
//!   state; their root visit counts are summed in tree order, so the move does not depend on
//!   which thread finishes first. The omniscient search grows its trees on the true state,
//!   each with its own random streams; the hidden one grows one per determinization of its
//!   Observation (ensemble determinization).

use std::thread;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch, Seat, State, Step,
    Stops, apply, next,
};

use crate::rollout::{RolloutPolicy, rollout};

/// What a search may spend on one decision. Iterations, not time, so a seed reproduces a move
/// on any machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Iterations per decision, shared as evenly as they go among the trees.
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
}

impl Default for Budget {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the search may know of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sight {
    /// The true state, hidden cards and library order included: cheating.
    Omniscient,
    /// Only its Observation: each tree searches a determinization of it.
    Hidden,
}

/// The UCT exploration constant, for rewards in [0, 1]. Against the greedy agent, 72 games a
/// constant at 100 iterations and 4 trees, the hidden search won 44% at 0.35, 51% at 0.7
/// and 43% at 1.4: the best of the three, though the intervals overlap.
const EXPLORATION: f64 = 0.7;

/// The searching agent.
#[derive(Clone, Debug)]
pub struct Mcts {
    table: GameTable,
    policy: RolloutPolicy,
    sight: Sight,
    budget: Budget,
    rng: SplitMix64,
}

impl Mcts {
    pub fn new(t: &GameTable, sight: Sight, budget: Budget, seed: u64) -> Self {
        Self {
            table: t.clone(),
            policy: RolloutPolicy::new(t),
            sight,
            budget,
            rng: SplitMix64::new(seed),
        }
    }

    /// The root visit counts of every tree, summed, one per candidate of `d`.
    pub fn visits(&mut self, obs: Observation<'_>, d: &Decision) -> Vec<u32> {
        let trees = self.budget.trees.max(1);
        let mut seeds = SplitMix64::new(self.rng.next_u64());
        let jobs: Vec<(u64, u32)> = (0..trees)
            .map(|i| {
                let share =
                    self.budget.iterations / trees + u32::from(i < self.budget.iterations % trees);
                (seeds.next_u64(), share)
            })
            .collect();
        let search = |&(seed, iterations): &(u64, u32)| {
            Tree::search(
                &self.table,
                &self.policy,
                self.sight,
                obs,
                d,
                seed,
                iterations,
            )
        };
        let counts: Vec<Vec<u32>> = if self.budget.parallel && jobs.len() > 1 {
            thread::scope(|scope| {
                let handles: Vec<_> = jobs
                    .iter()
                    .map(|job| scope.spawn(move || search(job)))
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("a search thread panicked"))
                    .collect()
            })
        } else {
            jobs.iter().map(search).collect()
        };
        let mut total = vec![0; d.len()];
        for tree in &counts {
            for (sum, n) in total.iter_mut().zip(tree) {
                *sum += n;
            }
        }
        total
    }
}

impl crate::Agent for Mcts {
    /// The candidate visited most across the trees, the first of equals. A decision with one
    /// candidate is never searched.
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
        if d.len() < 2 {
            return Choice(0);
        }
        let visits = self.visits(obs, d);
        let mut best = 0;
        for (i, &n) in visits.iter().enumerate() {
            if n > visits[best] {
                best = i;
            }
        }
        Choice(best as u8)
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
struct Tree<'a> {
    t: &'a GameTable,
    policy: &'a RolloutPolicy,
    nodes: Vec<Node>,
    rng: SplitMix64,
    x: Scratch,
}

impl<'a> Tree<'a> {
    /// Grows one tree for `iterations` from the position `obs` shows at decision `d`, and
    /// returns its root children's visit counts.
    fn search(
        t: &'a GameTable,
        policy: &'a RolloutPolicy,
        sight: Sight,
        obs: Observation<'_>,
        d: &Decision,
        seed: u64,
        iterations: u32,
    ) -> Vec<u32> {
        let mut rng = SplitMix64::new(seed);
        let root = match sight {
            Sight::Omniscient => {
                let mut s = *obs.unmasked_root();
                s.reseed(rng.next_u64());
                s
            }
            Sight::Hidden => obs.determinize(t, &mut rng),
        };
        let mut tree = Tree {
            t,
            policy,
            nodes: vec![Node::new(d.player)],
            rng,
            x: Scratch::new(),
        };
        tree.expand(0, d);
        for _ in 0..iterations {
            tree.iterate(&root);
        }
        let root = tree.nodes[0];
        tree.nodes[root.first as usize..][..usize::from(root.children)]
            .iter()
            .map(|n| n.visits)
            .collect()
    }

    /// One iteration: from a copy of the root, select by UCB1 down to a child not yet played,
    /// play it out, and score the game for every node on the way.
    fn iterate(&mut self, root: &State) {
        let mut s = *root;
        let mut path: [u32; 256] = [0; 256];
        let mut depth = 0;
        let mut node = 0;
        let outcome = loop {
            let child = self.select(node);
            path[depth] = child;
            depth += 1;
            let choice = Choice((child - self.nodes[node as usize].first) as u8);
            apply(&mut s, self.t, &mut self.x, &mut Replay, choice);
            if node == 0 {
                // The root decision was offered under the game's profile; below it the tree
                // plays under the smart stops. The payment profile stays, so a payment in
                // progress is never asked under another.
                s.set_profile(Profile {
                    stops: Stops::Smart,
                    breakpoint: None,
                    ..s.profile()
                });
            }
            if self.nodes[child as usize].visits == 0 || depth == path.len() {
                s.reseed(self.rng.next_u64());
                break rollout(&mut s, self.t, &mut self.x, self.policy);
            }
            match next(&mut s, self.t, &mut self.x, &mut Replay) {
                Step::Over(outcome) => break outcome,
                Step::Decide(below) => {
                    if self.nodes[child as usize].children == 0 {
                        self.expand(child, &below);
                    }
                    node = child;
                }
            }
        };
        self.nodes[0].visits += 1;
        for &n in &path[..depth] {
            let n = &mut self.nodes[n as usize];
            n.visits += 1;
            n.reward += score(outcome, n.mover);
        }
    }

    /// Gives `node` one child per candidate of its decision `d`.
    fn expand(&mut self, node: u32, d: &Decision) {
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

/// A finished game's reward for `player`: 1 for a win, 0 for a loss, 0.5 for a draw or a
/// stall.
fn score(outcome: Outcome, player: Seat) -> f64 {
    match outcome {
        Outcome::Win(winner) if winner == player => 1.0,
        Outcome::Win(_) => 0.0,
        Outcome::Draw | Outcome::Stalled(_) => 0.5,
    }
}
