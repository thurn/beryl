//! One tree of the search: its nodes, and growing it by iterations.

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, Decision, GameTable, Layout, Observation, Outcome, Profile, Replay, Scratch, Seat,
    SlotId, State, Step, Stops, apply, next, observe,
};
use std::sync::atomic::Ordering;

use super::{EXPLORATION, ROLLOUTS, Sight};
use crate::rollout::{RolloutPolicy, rollout};

/// One decision in a tree. Its children, one per candidate, are contiguous in the arena.
#[derive(Clone, Copy, Debug)]
pub(super) struct Node {
    pub(super) visits: u32,
    /// The sum of the rewards of the games through this node, for `mover`.
    pub(super) reward: f64,
    /// The player whose choice leads here.
    pub(super) mover: Seat,
    /// The first child's index, and how many there are; none until the node is expanded.
    pub(super) first: u32,
    pub(super) children: u8,
}

impl Node {
    pub(super) fn new(mover: Seat) -> Self {
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
pub(super) struct Tree<L: Layout> {
    pub(super) nodes: Vec<Node>,
    /// The node of the decision being searched.
    pub(super) root: u32,
    /// The position at the root: the true state or a determinization of it.
    pub(super) state: State<L>,
    pub(super) rng: SplitMix64,
    pub(super) x: Scratch<L>,
}

impl<L: Layout> Tree<L> {
    /// A tree of one expanded root, for the position `obs` shows at decision `d`.
    pub(super) fn new(
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

    pub(super) fn visits(&self) -> u32 {
        self.nodes[self.root as usize].visits
    }

    /// The engine steps the tree's scratch has counted over its life.
    pub(super) fn steps(&self) -> u64 {
        self.x.counts.decisions + self.x.counts.silent
    }

    pub(super) fn children(&self) -> &[Node] {
        let root = self.nodes[self.root as usize];
        &self.nodes[root.first as usize..][..usize::from(root.children)]
    }

    pub(super) fn counts(&self) -> Vec<u32> {
        self.children().iter().map(|n| n.visits).collect()
    }

    /// Whether the root's child for `c` has children of its own, so it can root a search.
    pub(super) fn expanded(&self, c: Choice) -> bool {
        self.children()[usize::from(c.0)].children > 0
    }

    pub(super) fn grow(&mut self, t: &GameTable, policy: &RolloutPolicy, iterations: u32) {
        for _ in 0..iterations {
            self.iterate(t, policy);
            ROLLOUTS.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Moves the root down to its child for `c`, if the game there reaches `d` with no other
    /// decision between and shows everything `obs` shows: had another player acted in the
    /// game, a tapped permanent, a counter or an entry on the stack would differ.
    pub(super) fn descend(
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
