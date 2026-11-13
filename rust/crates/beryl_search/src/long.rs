//! The long-think searcher: one Monte Carlo tree search with every axis the long-think
//! arena varied (docs/longthink-run-003.md; the arena is retired, docs/ai-strategy.md §8),
//! so that its entrants differ in one setting at a time. The ladder's champion and its
//! challengers (docs/ai-strategy.md §3, §6) are its settings. [`Mcts`](crate::Mcts), the
//! UI's quick opponent, stays as it is.
//!
//! - **Method.** [`Method::Pimc`] searches a fixed number of worlds, each sampled once at the
//!   root and searched by a tree of its own (perfect-information Monte Carlo, as `Mcts`).
//!   [`Method::Ismcts`] samples a fresh world every iteration into trees keyed by what the
//!   deciding player can tell apart (single-observer information-set MCTS, Cowling, Powley &
//!   Whitehouse 2012), so a million iterations see a million worlds, and the searcher never
//!   learns the order of its own library.
//! - **Sight.** What a world may keep of the truth: nothing hidden ([`Sight::Hidden`]),
//!   every hand ([`Sight::Hands`]) or everything ([`Sight::Omniscient`]).
//! - **Leaf.** A new node is scored by a rollout to the end of the game
//!   ([`Leaf::Rollout`]), one cut off after some turns and evaluated ([`Leaf::Cutoff`]), or
//!   the board evaluation alone ([`Leaf::Eval`]). Rollouts draw each move by a softmax over
//!   the greedy policy's scores ([`ROLLOUT_TEMPERATURE`]), so that a world the search knows
//!   still offers more than one game.
//! - **Knowledge.** The card values the rollout policy and the evaluation read
//!   ([`Knowledge`]).
//! - **Scores.** A win scores more the sooner it comes and a loss more the later
//!   ([`TURN_DISCOUNT`]), in every family, so that a decided position still has a best move.
//! - **Work.** A budget is engine steps, not iterations: every decision `next` returns or
//!   settles itself, in the tree or a rollout, plus what copying the root, dealing a world
//!   and an evaluation cost in steps' time ([`ITERATION_STEPS`], [`WORLD_STEPS`],
//!   [`EVAL_STEPS`]). So a full rollout (about 360
//!   steps from a mid-game position) and an evaluation (one) are charged what they cost,
//!   and every leaf compares at matched CPU, deterministically: a seed reproduces a move on
//!   any machine.
//!
//! Every tree's children are keyed by the action as the searcher tells it apart, not by its
//! index: an option naming a card it knew at the root keys by its slot; one naming a card it
//! sees only in this world (one it drew) or one the option shows (the opponent's spell from
//! hand) by the card; one naming a card it cannot see (a face-down permanent) by its slot.
//! So the same action in two worlds is one child, and the searcher never plans on a card it
//! would not know. A child counts the times it was available as well as visited, and
//! UCB1 reads the former (ISMCTS's availability count). A fixed world offers the same
//! actions each time, so there it is plain UCT.
//!
//! The trees grow in rounds, spread over threads; after each round the search stops once the
//! candidate visited most could not be overtaken at the rate iterations have cost so far. A
//! decision asked straight after the last, with nothing learned between, keeps its trees,
//! as `Mcts` does; their visits count toward the budget at what they cost.

use beryl_engine::{Large, Layout, SlotId, Small};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use beryl_engine::card::{FaceIx, Purpose};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Bitboard, Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome,
    Replay, Scratch, Seat, State, Step, Zone, apply, next, observe,
};

use crate::combat;
use crate::eval::evaluate;
use crate::mcts::{shows_the_same, smart};
use crate::rollout::{RolloutPolicy, Rules, Softmax, soft_rollout, soft_rollout_to};
use crate::values::Knowledge;
use crate::{Agent, AgentDecision, ExpertAgent, Search, Sight, Widen};

mod trace;
use trace::{Capture, Trace};
pub use trace::{TreeReport, Visits};

/// How the search treats what it cannot see.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Method {
    /// A tree per world, each world sampled once.
    Pimc { worlds: u16 },
    /// `trees` root-parallel trees, a fresh world each iteration.
    Ismcts { trees: u16 },
}

/// How a new node is scored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Leaf {
    /// A softmax rollout to the end of the game.
    Rollout,
    /// A softmax rollout to the first decision `turns` turns on, then the evaluation.
    Cutoff { turns: u8 },
    /// The evaluation of the position.
    Eval,
}

/// A searcher's settings, its budget aside: what a family of entrants shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Family {
    pub method: Method,
    pub sight: Sight,
    pub leaf: Leaf,
    pub knowledge: Knowledge,
    pub options: Options,
}

impl Family {
    /// The policy this family uses to play out its leaves.
    pub fn rollout_policy(self, t: &GameTable) -> RolloutPolicy {
        RolloutPolicy::with_rules(t, self.knowledge, self.options.rules())
    }
}

/// What a family adds to the plain search, each switchable on its own (epic by-7wl). None by
/// default: a family named without them plays as it always has.
// Independent switches by design, one a part of the family's name, each set or not by a rung.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Options {
    /// Where [`guarded`], the root defaults to greedy's answer (or the expert's with
    /// [`Options::expert`]). The search's own choice
    /// replaces it only by a clear lead ([`ANCHOR_MARGIN`], [`ANCHOR_ERRORS`]), measured tree
    /// by tree after the last quarter of the work goes to the two alone. Most of a search's
    /// choices lead the runner-up by less than noise; there, a sensible default beats a coin
    /// flip.
    pub anchor: bool,
    /// With [`Options::anchor`], use the live expert's answer as the default at every
    /// searched decision. Used for the yardstick in docs/ai-strategy.md §6.6.
    pub expert: bool,
    /// With [`Options::anchor`], [`guarded`] holds greedy's answer at attacker declarations,
    /// choices of up to a number of objects, and picks from the player's hand other than
    /// mulligan bottoms. At priority it holds against casts and activations scored at 0 or less.
    pub guard: bool,
    /// Below the root, greedy answers the questions asked while casting or resolving
    /// ([`answered_by_policy`]), so an option followed by a wide choice is not valued as if
    /// that choice were made at random.
    pub mid: bool,
    /// At the root, the candidate with the best tree-averaged mean replaces the one visited
    /// most where it clearly leads ([`by_means`]). The vote lets visits overrule the means:
    /// Jace, Wielder led an attack by 0.015 and lost on visits
    /// (`docs/incidents/2026-11-07-snapcaster-and-jace-wielder.md`).
    pub means: bool,
    /// Hold instant-speed plays until a useful window.
    pub hold: bool,
    /// Avoid spending resources on effects with no worthwhile targets.
    pub waste: bool,
    /// With the anchor, use the combat planner's answer at declarations and their targets.
    /// Below the root, the planner answers those decisions for both players on the root's turn.
    pub combat: bool,
    /// Try unvisited children in descending order of their greedy scores, including at the root.
    pub order: bool,
}

/// A searcher's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Settings {
    pub family: Family,
    /// Engine steps a decision may spend.
    pub work: u64,
    /// Threads the trees grow on, at most one per tree. The moves do not depend on it.
    pub threads: u32,
}

/// Engine steps charged for dealing a world: a state copy and a shuffle. Measured
/// 2026-11-06 over 20 greedy mirror games' positions: 1.27 µs a world, 0.5 µs a step.
pub const WORLD_STEPS: u64 = 3;

/// Engine steps charged for an evaluation: a pass over the battlefield and the hands.
/// Measured as [`WORLD_STEPS`]: 0.29 µs, rounded up to a step.
pub const EVAL_STEPS: u64 = 1;

/// Engine steps charged for an iteration's copy of the root, so that none costs nothing: an
/// action that ends the game at once counts no step of the engine's.
pub const ITERATION_STEPS: u64 = 1;

/// The softmax temperature of the search's rollouts, in the rollout policy's score points
/// ([`Softmax`]). Measured 2026-11-06 over 600 WU mirror games against greedy: at 10 the
/// softmax policy wins 50%, as strong as greedy, while 16 rollouts from a known position all
/// end alike in 44% of positions, against 75% for greedy rollouts; at 20 it wins 45%.
pub const ROLLOUT_TEMPERATURE: f64 = 10.0;

/// The UCT exploration constant, as `Mcts`'s.
const EXPLORATION: f64 = 0.7;

/// The rounds a decision's work is spent in, the stop checked after each.
const ROUNDS: u64 = 16;

/// The rounds an anchored search grows freely before the rest goes to its choice and
/// the default answer alone.
const FREE_ROUNDS: u64 = ROUNDS * 3 / 4;

/// The least lead in mean reward over the default answer that lets an anchored search play its
/// own choice instead. Below it, the rollouts' small differences are not trusted however
/// many there are: the Deep Seer incident's Oust on its own token led passing by 0.006.
/// Measured by `beryl_cli anchor-replay --guarded` over run-003's games of
/// `pimc18/omniscient/rollout/tuned@10M` (42 games, 2026-11-07), every decision it would
/// switch searched again from a fresh seed: at 0.02 the anchor switches 0.4 decisions a game
/// at a cost of 0.005 ± 0.001 a game by the search's own estimate; at 0.04, 0.7 for 0.013.
/// Measured before [`TURN_DISCOUNT`], which shrinks leads by a tenth to a quarter a few turns
/// out, so the margin now asks a little more.
pub const ANCHOR_MARGIN: f64 = 0.02;

/// The standard errors, across the trees, by which an anchored search's lead over greedy's
/// answer must clear zero.
pub const ANCHOR_ERRORS: f64 = 2.0;

/// The least share of the most visited candidate's visits that lets another be chosen by its
/// mean ([`Options::means`]): a mean resting on fewer visits is too noisy to overrule the vote.
const MEANS_FLOOR: u32 = 10;

/// The standard errors, across the trees, by which a candidate's lead in mean over the one
/// visited most must clear zero for [`Options::means`] to play it.
const MEANS_ERRORS: f64 = 2.0;

/// No node.
const NONE: u32 = u32::MAX;

/// How much less a game's result counts each turn it is put off: a search scores a win
/// `0.5 + 0.5 * TURN_DISCOUNT^t` and a loss `0.5 - 0.5 * TURN_DISCOUNT^t`, `t` the turns since
/// its tree's first root. Without it a decided position offers no best move, every move
/// winning (or losing) in nearly every rollout, and noise chose: in run-003, 45% of
/// `pimc18/omniscient/rollout/tuned@10M`'s searched decisions came
/// in positions it rated above 0.95 or below 0.05. So it now closes a won game out and drags
/// a lost one on. Measured by `beryl_cli agreement` over 60 of those positions and 60
/// contested ones (2026-11-07, two seeds each at 10M): the seeds chose alike in 60% of
/// decided positions undiscounted, 78% at 0.98 and 80% at 0.95, and in 78%, 77% and 90% of
/// contested ones. 0.98 rather than 0.95 because the stronger discount trades more of a
/// close game's chances for speed: at 0.95 a line winning 63% in 14 turns loses to one winning
/// 60% in 8.
pub const TURN_DISCOUNT: f64 = 0.98;

impl fmt::Display for Family {
    /// `ismcts18/hidden/rollout/tuned`: method, sight, leaf, knowledge, then any options, as
    /// `pimc18/omniscient/rollout/tuned/anchor+mid`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.method {
            Method::Pimc { worlds } => write!(f, "pimc{worlds}")?,
            Method::Ismcts { trees } => write!(f, "ismcts{trees}")?,
        }
        let sight = match self.sight {
            Sight::Hidden => "hidden",
            Sight::Hands => "hands",
            Sight::Omniscient => "omniscient",
        };
        let knowledge = match self.knowledge {
            Knowledge::Estimate => "estimate",
            Knowledge::Tuned => "tuned",
        };
        write!(f, "/{sight}/")?;
        match self.leaf {
            Leaf::Rollout => write!(f, "rollout")?,
            Leaf::Cutoff { turns } => write!(f, "cut{turns}")?,
            Leaf::Eval => write!(f, "eval")?,
        }
        write!(f, "/{knowledge}")?;
        let options = self.options.to_name();
        if options.is_empty() {
            Ok(())
        } else {
            write!(f, "/{options}")
        }
    }
}

impl FromStr for Family {
    type Err = String;

    fn from_str(s: &str) -> Result<Family, String> {
        let bad = || {
            format!(
                "{s:?} is no family: method/sight/leaf/knowledge[/options], as \
                 ismcts18/hidden/rollout/tuned (pimcN or ismctsN; hidden, hands or omniscient; \
                 rollout, cutN or eval; estimate or tuned; options anchor, expert, guard, mid, \
                 means, hold, waste, combat and order joined by + in that order; \
                 expert and guard require anchor)"
            )
        };
        let parts: Vec<&str> = s.split('/').collect();
        let (&[method, sight, leaf, knowledge], options) = parts.split_at(4.min(parts.len()))
        else {
            return Err(bad());
        };
        let options = match options {
            [] => Options::default(),
            [named] => {
                let mut o = Options::default();
                for name in named.split('+') {
                    let flag = match name {
                        "anchor" => &mut o.anchor,
                        "expert" => &mut o.expert,
                        "guard" => &mut o.guard,
                        "mid" => &mut o.mid,
                        "means" => &mut o.means,
                        "hold" => &mut o.hold,
                        "waste" => &mut o.waste,
                        "combat" => &mut o.combat,
                        "order" => &mut o.order,
                        _ => return Err(bad()),
                    };
                    if *flag {
                        return Err(bad());
                    }
                    *flag = true;
                }
                // One spelling per family, so that a name read back prints as it was given.
                if o.to_name() != *named || ((o.expert || o.guard) && !o.anchor) {
                    return Err(bad());
                }
                o
            }
            _ => return Err(bad()),
        };
        let count = |rest: &str| rest.parse::<u16>().ok().filter(|&n| n > 0);
        let method = if let Some(n) = method.strip_prefix("pimc").and_then(count) {
            Method::Pimc { worlds: n }
        } else if let Some(n) = method.strip_prefix("ismcts").and_then(count) {
            Method::Ismcts { trees: n }
        } else {
            return Err(bad());
        };
        let sight = match sight {
            "hidden" => Sight::Hidden,
            "hands" => Sight::Hands,
            "omniscient" => Sight::Omniscient,
            _ => return Err(bad()),
        };
        let leaf = match leaf {
            "rollout" => Leaf::Rollout,
            "eval" => Leaf::Eval,
            cut => match cut.strip_prefix("cut").and_then(|n| n.parse::<u8>().ok()) {
                Some(turns) if turns > 0 => Leaf::Cutoff { turns },
                _ => return Err(bad()),
            },
        };
        let knowledge = match knowledge {
            "estimate" => Knowledge::Estimate,
            "tuned" => Knowledge::Tuned,
            _ => return Err(bad()),
        };
        Ok(Family {
            method,
            sight,
            leaf,
            knowledge,
            options,
        })
    }
}

impl Options {
    /// The options as a family's name spells them, joined by `+` in this order:
    /// `anchor+expert+guard+mid+means+hold+waste+combat+order`; empty for none.
    fn to_name(self) -> String {
        [
            (self.anchor, "anchor"),
            (self.expert, "expert"),
            (self.guard, "guard"),
            (self.mid, "mid"),
            (self.means, "means"),
            (self.hold, "hold"),
            (self.waste, "waste"),
            (self.combat, "combat"),
            (self.order, "order"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect::<Vec<_>>()
        .join("+")
    }

    fn rules(self) -> Rules {
        Rules {
            hold: self.hold,
            waste: self.waste,
        }
    }
}

/// The long-think searching agent.
#[derive(Debug)]
pub struct LongSearch<L: Layout = Small> {
    table: GameTable,
    policy: RolloutPolicy,
    expert: Option<ExpertAgent<L>>,
    combat_plan: combat::Plan<L>,
    settings: Settings,
    seed: u64,
    /// The next decision's index among the agent's own.
    decision: AgentDecision,
    kept: Option<Kept<L>>,
    last: Option<Search>,
}

/// The trees of a decision just answered, while the next may reuse them.
#[derive(Debug)]
struct Kept<L: Layout> {
    trees: Vec<Tree<L>>,
    chain: AgentDecision,
    choice: Choice,
    /// Each slot's incarnation, and whether the deciding player knew its card, then.
    seen: Vec<(u16, bool)>,
    /// The card the choice names, which it may move.
    moves: Option<L::Slot>,
}

impl<L: Layout> LongSearch<L> {
    pub fn new(t: &GameTable, settings: Settings, seed: u64) -> Self {
        Self::resumed(t, settings, seed, AgentDecision(0))
    }

    /// The agent as it stands before its own decision `decision`, for a game resumed midway
    /// (as [`crate::Mcts::resumed`]).
    pub fn resumed(t: &GameTable, settings: Settings, seed: u64, decision: AgentDecision) -> Self {
        let family = settings.family;
        Self {
            table: t.clone(),
            policy: family.rollout_policy(t),
            expert: family.options.expert.then(|| ExpertAgent::new(t, seed)),
            combat_plan: combat::Plan::default(),
            settings,
            seed,
            decision,
            kept: None,
            last: None,
        }
    }

    /// The trees for decision `d`, seeded from the agent's seed and the decision's index.
    fn plant(
        &self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        decision: AgentDecision,
    ) -> Vec<Tree<L>> {
        let mut seeds = SplitMix64::new(
            SplitMix64::new(
                self.seed
                    .wrapping_add(u64::from(decision.0).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
            )
            .next_u64(),
        );
        let (n, resample) = match self.settings.family.method {
            Method::Pimc { worlds } => (worlds, false),
            Method::Ismcts { trees } => (trees, true),
        };
        let known: Bitboard<L::Slot> = L::Slot::all().filter(|&slot| obs.knows(slot)).collect();
        (0..n.max(1))
            .map(|_| {
                Tree::new(
                    &self.table,
                    self.settings.family,
                    obs,
                    d,
                    known,
                    resample,
                    seeds.next_u64(),
                )
            })
            .collect()
    }

    /// The kept trees moved down to the chosen child, if `d` may reuse them: as
    /// [`crate::Mcts`]'s, no card changed zones and the player learned nothing since, and for
    /// a fixed world, it reached `d` straight from that child showing what the game shows.
    fn descend(
        &self,
        kept: Kept<L>,
        obs: Observation<'_, L>,
        d: &Decision<L>,
    ) -> Option<Vec<Tree<L>>> {
        // CR 400.7: "An object that moves from one zone to another becomes a new object
        // with no memory of, or relation to, its previous existence", so a new incarnation is
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

    /// Grows `trees` on decision `d` to the budget, in rounds, until the leader is settled.
    /// With the default answer `anchor`, the first [`FREE_ROUNDS`] grow as ever; then, if the
    /// leader differs from the default and `guards` holds, the rest of the work goes to the two
    /// alone, so that the choice between them is not made against a starved estimate.
    /// Returns the iterations run, the work they spent and the candidate checked against
    /// the default answer that way, if one was.
    fn grow(
        &self,
        trees: &mut [Tree<L>],
        anchor: Option<Choice>,
        guards: impl Fn(Choice) -> bool,
    ) -> (u64, u64, Option<Choice>) {
        let n = trees.len() as u64;
        let work = self.settings.work;
        let shares: Vec<u64> = (0..n).map(|i| work / n + u64::from(i < work % n)).collect();
        // What each tree's kept visits are worth, at what its iterations have cost.
        let credit: Vec<u64> = trees
            .iter()
            .zip(&shares)
            .map(|(tree, &share)| (u64::from(tree.visits()) * tree.cost()).min(share))
            .collect();
        let start: Vec<(u64, u64)> = trees.iter().map(|t| (t.work, t.iterations)).collect();
        let plan = Plan {
            shares: &shares,
            credit: &credit,
            start: &start,
            threads: (self.settings.threads.max(1) as usize).min(trees.len()),
        };
        let mut checked = None;
        match anchor {
            None => self.rounds(trees, &plan, 1..=ROUNDS, true),
            Some(g) => {
                self.rounds(trees, &plan, 1..=FREE_ROUNDS, true);
                let (b, g) = (self.pick(trees), usize::from(g.0));
                if b == g || !guards(Choice(b as u16)) {
                    if !settled(trees, &shares, &credit, &start) {
                        self.rounds(trees, &plan, FREE_ROUNDS + 1..=ROUNDS, true);
                    }
                } else {
                    for tree in trees.iter_mut() {
                        tree.allowed = Some([b, g]);
                    }
                    self.rounds(trees, &plan, FREE_ROUNDS + 1..=ROUNDS, false);
                    for tree in trees.iter_mut() {
                        tree.allowed = None;
                    }
                    checked = Some(Choice(b as u16));
                }
            }
        }
        let (iterations, work) = trees
            .iter()
            .zip(&start)
            .fold((0, 0), |(i, w), (tree, &(w0, i0))| {
                (i + tree.iterations - i0, w + tree.work - w0)
            });
        (iterations, work, checked)
    }

    /// The root candidate the search would play before any anchor: the one visited most, or
    /// with [`Options::means`], the one [`by_means`] picks.
    fn pick(&self, trees: &[Tree<L>]) -> usize {
        if self.settings.family.options.means {
            by_means(&root_stats(trees))
        } else {
            let (visits, rewards) = totals(trees);
            leader(&visits, &rewards)
        }
    }

    /// For research: grows fresh trees on decision `d` for the whole budget with their roots
    /// held to candidates `a` and `b`, and returns `a`'s lead over `b` tree by tree, as an
    /// anchored search checks it (`a` as its choice, `b` as greedy's answer). Unlike the lead
    /// a search reports for the candidate it visited most, it carries no bias from having
    /// been chosen. Keeps nothing.
    pub fn duel(
        &self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        a: Choice,
        b: Choice,
    ) -> AnchorCheck {
        let mut trees = self.plant(obs, d, self.decision);
        let n = trees.len() as u64;
        let work = self.settings.work;
        let shares: Vec<u64> = (0..n).map(|i| work / n + u64::from(i < work % n)).collect();
        let credit = vec![0; trees.len()];
        let start: Vec<(u64, u64)> = trees.iter().map(|t| (t.work, t.iterations)).collect();
        for tree in &mut trees {
            tree.allowed = Some([usize::from(a.0), usize::from(b.0)]);
        }
        let plan = Plan {
            shares: &shares,
            credit: &credit,
            start: &start,
            threads: (self.settings.threads.max(1) as usize).min(trees.len()),
        };
        self.rounds(&mut trees, &plan, 1..=ROUNDS, false);
        check(&trees, a, b)
    }

    /// Grows `trees` through `rounds` of `plan`, each to its share of the work so far, and
    /// with `stop`, no further once the leader is settled.
    fn rounds(
        &self,
        trees: &mut [Tree<L>],
        plan: &Plan<'_>,
        rounds: std::ops::RangeInclusive<u64>,
        stop: bool,
    ) {
        let (table, policy) = (&self.table, &self.policy);
        for round in rounds {
            let targets: Vec<u64> = plan
                .shares
                .iter()
                .zip(plan.credit)
                .zip(plan.start)
                .map(|((&share, &credit), &(work, _))| {
                    (work + (share * round / ROUNDS)).saturating_sub(credit)
                })
                .collect();
            if plan.threads <= 1 {
                for (tree, &target) in trees.iter_mut().zip(&targets) {
                    tree.grow_to(table, policy, target);
                }
            } else {
                grow_parallel(table, policy, trees, &targets, plan.threads);
            }
            if stop && round < ROUNDS && settled(trees, plan.shares, plan.credit, plan.start) {
                break;
            }
        }
    }
}

/// How a decision's work is shared among its trees: each tree's share, the credit its kept
/// visits bring, its work and iterations when the decision began, and the threads.
struct Plan<'a> {
    shares: &'a [u64],
    credit: &'a [u64],
    start: &'a [(u64, u64)],
    threads: usize,
}

/// An anchored search's check of its choice against the default answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorCheck {
    /// The default answer, from the combat planner, live expert or greedy policy.
    pub greedy: Choice,
    /// The search's own choice: the candidate visited most.
    pub search: Choice,
    /// The search's choice's mean reward less the default's, averaged over the trees that visited
    /// both, and the standard error of that average across them.
    pub lead: f64,
    pub error: f64,
    /// The trees the lead was averaged over.
    pub trees: u16,
}

impl AnchorCheck {
    /// Whether the search's choice leads the default answer clearly enough to be played instead:
    /// by [`ANCHOR_MARGIN`], and by [`ANCHOR_ERRORS`] standard errors where more than one tree
    /// measured it.
    #[must_use]
    pub fn overrides(&self) -> bool {
        let noise = if self.trees > 1 {
            ANCHOR_ERRORS * self.error
        } else {
            0.0
        };
        self.search != self.greedy && self.trees > 0 && self.lead > ANCHOR_MARGIN.max(noise)
    }

    /// The candidate an anchored search plays.
    #[must_use]
    pub fn played(&self) -> Choice {
        if self.overrides() {
            self.search
        } else {
            self.greedy
        }
    }
}

/// The check of choice `search` against the default answer across `trees`.
fn check<L: Layout>(trees: &[Tree<L>], search: Choice, default: Choice) -> AnchorCheck {
    let leads: Vec<f64> = trees
        .iter()
        .filter_map(|tree| {
            let mean = |c: Choice| {
                let n = tree.child(tree.root, tree.root_keys[usize::from(c.0)])?;
                let n = &tree.nodes[n as usize];
                (n.visits > 0).then(|| n.reward / f64::from(n.visits))
            };
            Some(mean(search)? - mean(default)?)
        })
        .collect();
    let (lead, error) = mean_and_error(&leads);
    AnchorCheck {
        greedy: default,
        search,
        lead,
        error,
        trees: u16::try_from(leads.len()).unwrap_or(u16::MAX),
    }
}

/// The mean of `xs` and its standard error: 0 for both when empty, the error 0 for one.
fn mean_and_error(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if xs.is_empty() {
        return (0.0, 0.0);
    }
    let mean = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (mean, 0.0);
    }
    let variance = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, (variance / n).sqrt())
}

/// Grows each tree to its target on `threads` threads, each taking the next tree not yet
/// grown: a tree's growth depends on nothing but itself, so neither does the move.
fn grow_parallel<L: Layout>(
    t: &GameTable,
    policy: &RolloutPolicy,
    trees: &mut [Tree<L>],
    targets: &[u64],
    threads: usize,
) {
    let queue: Vec<Mutex<&mut Tree<L>>> = trees.iter_mut().map(Mutex::new).collect();
    let taken = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = taken.fetch_add(1, Ordering::Relaxed);
                    let Some(tree) = queue.get(i) else { break };
                    tree.lock().expect("a tree").grow_to(t, policy, targets[i]);
                }
            });
        }
    });
}

/// Whether the candidate visited most is settled: no other could overtake it if every
/// iteration the remaining work buys, at what this decision's iterations have cost each
/// tree, went its way. Equals are told apart by their mean, so every other must stay
/// strictly behind.
fn settled<L: Layout>(
    trees: &[Tree<L>],
    shares: &[u64],
    credit: &[u64],
    start: &[(u64, u64)],
) -> bool {
    let mut left = 0u64;
    for ((tree, &share), (&credit, &(w0, i0))) in
        trees.iter().zip(shares).zip(credit.iter().zip(start))
    {
        let spent = tree.work.saturating_sub(w0);
        let iterations = tree.iterations - i0;
        let remaining = share.saturating_sub(credit).saturating_sub(spent);
        let cost = if iterations > 0 {
            spent.div_ceil(iterations).max(1)
        } else {
            1
        };
        left += remaining.div_ceil(cost);
    }
    let (visits, rewards) = totals(trees);
    let best = leader(&visits, &rewards);
    visits
        .iter()
        .enumerate()
        .all(|(i, &n)| i == best || u64::from(visits[best]) > u64::from(n) + left)
}

/// The candidate visited most; of equals, the one with the best mean reward, then the first.
/// Not the first alone: that is passing, in a priority decision, and visits tie most often
/// where the search cannot tell its candidates apart.
fn leader(visits: &[u32], rewards: &[f64]) -> usize {
    let mean = |i: usize| {
        if visits[i] == 0 {
            f64::NEG_INFINITY
        } else {
            rewards[i] / f64::from(visits[i])
        }
    };
    let mut best = 0;
    for (i, &n) in visits.iter().enumerate() {
        if n > visits[best] || (n == visits[best] && mean(i) > mean(best)) {
            best = i;
        }
    }
    best
}

/// With [`Options::means`], the root candidate to play given each tree's root visits and
/// rewards per candidate (`stats[tree][candidate]`): the one with the best tree-averaged mean
/// among those with at least a [`MEANS_FLOOR`]th of the most visited candidate's visits,
/// unless the most visited candidate's mean is within the noise across trees of it, in which
/// case the most visited. The noise is [`MEANS_ERRORS`] standard errors of the lead, tree by
/// tree, over the trees that visited both, and at least two must have: one tree's lead has no
/// measured noise. Where the vote and the means agree, it is [`leader`]'s choice.
fn by_means(stats: &[Vec<Option<(u32, f64)>>]) -> usize {
    let n = stats.first().map_or(0, Vec::len);
    let mut visits = vec![0u32; n];
    let mut rewards = vec![0.0; n];
    for tree in stats {
        for (i, &(v, r)) in tree
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Some((i, c.as_ref()?)))
        {
            visits[i] += v;
            rewards[i] += r;
        }
    }
    let voted = leader(&visits, &rewards);
    let means = crate::mcts::tree_means(n, stats.iter().map(|t| t.iter().copied()));
    let floor = visits.get(voted).map_or(0, |&v| v.div_ceil(MEANS_FLOOR));
    let mut best = voted;
    for i in 0..n {
        if visits[i] >= floor.max(1)
            && let (Some(m), Some(b)) = (means[i], means[best])
            && m > b
        {
            best = i;
        }
    }
    if best == voted {
        return voted;
    }
    let mean = |c: &Option<(u32, f64)>| c.filter(|&(v, _)| v > 0).map(|(v, r)| r / f64::from(v));
    let leads: Vec<f64> = stats
        .iter()
        .filter_map(|tree| Some(mean(&tree[best])? - mean(&tree[voted])?))
        .collect();
    let (lead, error) = mean_and_error(&leads);
    if leads.len() >= 2 && lead > MEANS_ERRORS * error && lead > 0.0 {
        best
    } else {
        voted
    }
}

/// Each tree's root visits and rewards per candidate of the root decision, `None` for a
/// candidate it never expanded.
fn root_stats<L: Layout>(trees: &[Tree<L>]) -> Vec<Vec<Option<(u32, f64)>>> {
    trees
        .iter()
        .map(|tree| {
            tree.root_children()
                .into_iter()
                .map(|node| {
                    node.map(|n| (tree.nodes[n as usize].visits, tree.nodes[n as usize].reward))
                })
                .collect()
        })
        .collect()
}

/// The root visits and rewards per candidate of the root decision, summed across `trees` in
/// order.
fn totals<L: Layout>(trees: &[Tree<L>]) -> (Vec<u32>, Vec<f64>) {
    let n = trees.first().map_or(0, |t| t.decision.len());
    let mut visits = vec![0; n];
    let mut rewards = vec![0.0; n];
    for tree in trees {
        for (i, node) in tree.root_children().into_iter().enumerate() {
            if let Some(node) = node {
                visits[i] += tree.nodes[node as usize].visits;
                rewards[i] += tree.nodes[node as usize].reward;
            }
        }
    }
    (visits, rewards)
}

/// The card a choice names, which answering it may move.
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

/// The same search in [`Large`], at the same place among its own decisions. The trees it kept
/// hold `Small` states, so they are dropped: its next search plants fresh ones.
impl Widen for LongSearch<Small> {
    type Large = LongSearch<Large>;

    fn widen(&self) -> LongSearch<Large> {
        LongSearch {
            table: self.table.clone(),
            policy: self.policy.clone(),
            expert: self.expert.as_ref().map(Widen::widen),
            combat_plan: self.combat_plan.widen(),
            settings: self.settings,
            seed: self.seed,
            decision: self.decision,
            kept: None,
            last: None,
        }
    }
}

impl<L: Layout> LongSearch<L> {
    fn choose_traced(
        &mut self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        at: AgentDecision,
        capture: Option<&mut Capture<L>>,
    ) -> Choice {
        let decision = at;
        if at != self.decision {
            // A game resumed, or taken back by an undo: what was kept from the decisions
            // before belongs to another line of play.
            self.kept = None;
            self.combat_plan = combat::Plan::default();
            if self.expert.is_some() {
                self.expert = Some(ExpertAgent::new(&self.table, self.seed));
            }
        }
        self.decision = decision.next();
        self.last = None;
        let kept = self.kept.take();
        if d.len() < 2 {
            return Choice(0);
        }
        if d.kind == DecisionKind::Priority
            && obs.stack().last().is_some_and(|e| e.controller == d.player)
        {
            return Choice(0);
        }
        let chain = kept.as_ref().map(|k| k.chain);
        let (mut trees, chain) = match kept.and_then(|k| self.descend(k, obs, d)) {
            Some(trees) => (trees, chain.expect("kept trees")),
            None => (self.plant(obs, d, decision), decision),
        };
        let inherited = trees.iter().map(Tree::visits).sum();
        // The default answers from the player's observation, independently of the search trees.
        let options = self.settings.family.options;
        let combat_default = options.anchor && options.combat && combat::handles(d.kind);
        let default = options.anchor.then(|| match &mut self.expert {
            _ if combat_default => {
                // Use a stream separate from the worlds planted for the search.
                let mut rng = SplitMix64::new(self.seed ^ !u64::from(decision.0));
                let start = obs.determinize(&self.table, &mut rng);
                combat::Sim::<_, combat::General>::new(
                    &self.table,
                    &self.policy,
                    &mut Scratch::unchecked(),
                    d.player,
                    &mut self.combat_plan,
                )
                .choose(&start, d)
            }
            Some(expert) => expert.choose(obs, d, decision),
            None => self.policy.choose(&self.table, obs, d),
        });
        let guards = |b: Choice| {
            default.is_some_and(|g| guarded(&self.table, &self.policy, obs, d, g, b, options))
        };
        if let Some(capture) = capture.as_ref() {
            capture.start(&mut trees);
        }
        let (iterations, work, checked) = self.grow(&mut trees, default, guards);
        if let Some(capture) = capture {
            capture.finish(&mut trees);
        }
        let (visits, rewards) = totals(&trees);
        let best = Choice(self.pick(&trees) as u16);
        let means = crate::mcts::tree_means(visits.len(), root_stats(&trees));
        // The candidate checked against the default after the two had the last of the work
        // to themselves, whichever then has more visits; else the leader, if guarded.
        let anchor = default.and_then(|g| {
            let search = checked.or(guards(best).then_some(best))?;
            Some(check(&trees, search, g))
        });
        let choice = anchor.map_or(best, |a| a.played());
        if combat_default && default != Some(choice) {
            // The rest of the plan assumed that we made the declaration it chose.
            self.combat_plan = combat::Plan::default();
        }
        self.last = Some(Search {
            decision,
            chain,
            rollouts: u32::try_from(iterations).unwrap_or(u32::MAX),
            inherited,
            work,
            visits,
            rewards,
            means,
            anchor,
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
}

impl<L: Layout> Agent<L> for LongSearch<L> {
    /// The candidate visited most across the trees, of equals the best by mean; with
    /// [`Options::means`], [`by_means`]'s; with [`Options::anchor`], the guarded default
    /// answer unless it is clearly beaten. A decision with one
    /// candidate is never searched, nor is passing with one's own object on top of the stack.
    /// Asked a decision other than the one after its last (a game resumed, or taken back by an
    /// undo), it forgets its kept trees and combat plan and searches afresh.
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>, at: AgentDecision) -> Choice {
        self.choose_traced(obs, d, at, None)
    }

    fn last_search(&self) -> Option<&Search> {
        self.last.as_ref()
    }

    fn resume_point(&self) -> Option<AgentDecision> {
        let options = self.settings.family.options;
        if self.expert.is_some() || (options.anchor && options.combat) {
            // Match replay must rebuild the combat plan, the expert's included.
            return Some(AgentDecision(0));
        }
        Some(self.kept.as_ref().map_or(self.decision, |k| k.chain))
    }
}

/// One node: an action, by its key, and the games through it.
#[derive(Clone, Copy, Debug)]
struct Node {
    visits: u32,
    /// The times its parent was visited with this action available.
    avail: u32,
    /// The sum of the rewards of the games through it, for `mover`.
    reward: f64,
    key: u64,
    /// The player whose action this is.
    mover: Seat,
    first: u32,
    sibling: u32,
}

impl Node {
    fn new(key: u64, mover: Seat) -> Self {
        Node {
            visits: 0,
            avail: 0,
            reward: 0.0,
            key,
            mover,
            first: NONE,
            sibling: NONE,
        }
    }
}

/// One tree.
#[derive(Debug)]
struct Tree<L: Layout> {
    nodes: Vec<Node>,
    root: u32,
    /// The root decision.
    decision: Decision<L>,
    /// Each root candidate's key.
    root_keys: Vec<u64>,
    /// A fixed world's state at the root; for a resampling tree, the true state, which each
    /// iteration deals a world from as `sight` allows.
    state: State<L>,
    viewer: Seat,
    family: Family,
    resample: bool,
    /// The slots the searcher knew at the first root, whose options key by slot.
    known: Bitboard<L::Slot>,
    rng: SplitMix64,
    x: Scratch<L>,
    /// Engine steps and iterations over the tree's life.
    work: u64,
    iterations: u64,
    /// Steps charged beyond the scratch's counts: worlds and evaluations.
    extra: u64,
    /// While an anchored search checks its choice, the two root candidates it plays.
    allowed: Option<[usize; 2]>,
    /// The turn its first root was asked in, from which a finished game's turns count.
    origin: u16,
    /// Optional diagnostic counts of the decisions seen in the iteration's actual world.
    trace: Option<Box<Trace<L>>>,
}

impl<L: Layout> Tree<L> {
    fn new(
        t: &GameTable,
        family: Family,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        known: Bitboard<L::Slot>,
        resample: bool,
        seed: u64,
    ) -> Self {
        let mut rng = SplitMix64::new(seed);
        let truth = obs.unmasked_root().clone();
        let mut tree = Tree {
            nodes: vec![Node::new(0, d.player)],
            root: 0,
            decision: d.clone(),
            root_keys: Vec::new(),
            state: truth,
            viewer: d.player,
            family,
            resample,
            known,
            rng: SplitMix64::new(0),
            x: Scratch::unchecked(),
            work: 0,
            iterations: 0,
            extra: 0,
            allowed: None,
            origin: 0,
            trace: None,
        };
        tree.origin = tree.state.turn().number();
        if !resample {
            tree.state = tree.deal(t, &mut rng);
            tree.extra += WORLD_STEPS;
        }
        tree.rng = rng;
        tree.root_keys = tree.root_keys(t, d);
        tree
    }

    /// A world for an iteration: from the true root, what the sight keeps and the rest dealt.
    fn deal(&self, t: &GameTable, rng: &mut SplitMix64) -> State<L> {
        let obs = Observation::new(&self.state, self.viewer);
        match self.family.sight {
            Sight::Hidden => obs.determinize(t, rng),
            Sight::Hands => obs.determinize_libraries(t, rng),
            Sight::Omniscient => {
                let mut s = self.state.clone();
                s.reseed(rng.next_u64());
                s
            }
        }
    }

    /// The steps spent over the tree's life.
    fn spent(&self) -> u64 {
        self.x.counts.decisions + self.x.counts.silent + self.extra
    }

    /// Root visits: those of the root's children available to its decision.
    fn visits(&self) -> u32 {
        self.root_children()
            .into_iter()
            .flatten()
            .map(|n| self.nodes[n as usize].visits)
            .sum()
    }

    /// Mean steps an iteration has cost over the tree's life, at least one.
    fn cost(&self) -> u64 {
        if self.iterations == 0 {
            1
        } else {
            self.work.div_ceil(self.iterations).max(1)
        }
    }

    /// The root's child for each candidate of its decision, if it has one.
    fn root_children(&self) -> Vec<Option<u32>> {
        self.root_keys
            .iter()
            .map(|&k| self.child(self.root, k))
            .collect()
    }

    fn child(&self, node: u32, key: u64) -> Option<u32> {
        let mut c = self.nodes[node as usize].first;
        while c != NONE {
            if self.nodes[c as usize].key == key {
                return Some(c);
            }
            c = self.nodes[c as usize].sibling;
        }
        None
    }

    /// Whether the root's child for `c` has children of its own, so it can root a search.
    fn expanded(&self, c: Choice) -> bool {
        self.child(self.root, self.root_keys[usize::from(c.0)])
            .is_some_and(|n| self.nodes[n as usize].first != NONE)
    }

    /// Moves the root down to its child for `c`, if `d` may follow it.
    fn descend(
        &mut self,
        t: &GameTable,
        c: Choice,
        obs: Observation<'_, L>,
        d: &Decision<L>,
    ) -> bool {
        let Some(child) = self.child(self.root, self.root_keys[usize::from(c.0)]) else {
            return false;
        };
        if self.resample {
            // Only a question that follows the choice within its own action (a spell's
            // targets, its payment) keeps the tree: before a priority decision the opponent
            // may have had one of their own, which a resampling tree cannot rule out without
            // reading what they hold.
            if d.kind == DecisionKind::Priority {
                return false;
            }
            self.state = obs.unmasked_root().clone();
        } else {
            let mut s = self.state.clone();
            self.x.counts.copied();
            apply(&mut s, t, &mut self.x, &mut Replay, c);
            smart(&mut s);
            if next(&mut s, t, &mut self.x, &mut Replay) != Step::Decide(d.clone()) {
                return false;
            }
            if !shows_the_same(observe(&s, t, &mut self.x, d.player), obs)
                || !self.keeps_to(&s, obs.unmasked_root())
            {
                return false;
            }
            self.state = s;
        }
        self.root = child;
        self.decision = d.clone();
        self.root_keys = self.root_keys(t, d);
        self.work = self.spent();
        true
    }

    /// Whether fixed world `s`, played on to the truth's decision, still holds what the sight
    /// takes from the truth: the order of each library, which is public, and the cards the
    /// sight shows, every one for an omniscient world and those in hand for one that sees
    /// hands. The viewer's own view does not tell: a library shuffled on the way
    /// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a
    /// library or a face-down pile of cards, randomize the cards within it so that no player
    /// knows their order.") is shuffled by the world's random streams, not the game's, and a
    /// card the viewer does not know may land anywhere.
    fn keeps_to(&self, s: &State<L>, truth: &State<L>) -> bool {
        let shown = |slot: L::Slot| match self.family.sight {
            Sight::Hidden => false,
            Sight::Hands => truth.object(slot).zone() == Zone::Hand,
            Sight::Omniscient => true,
        };
        Seat::BOTH.iter().all(|&p| s.library(p) == truth.library(p))
            && L::Slot::all()
                .all(|slot| !shown(slot) || s.card_behind(slot) == truth.card_behind(slot))
    }

    /// The keys of root decision `d`'s candidates, read in a world: the fixed one, or one
    /// dealt for the purpose, so that no key is read off the true state.
    fn root_keys(&self, t: &GameTable, d: &Decision<L>) -> Vec<u64> {
        if self.resample {
            let world = self.deal(t, &mut self.rng.clone());
            keys(&world, d, self.known, self.viewer)
        } else {
            keys(&self.state, d, self.known, self.viewer)
        }
    }

    /// Grows the tree until its work reaches `target`.
    fn grow_to(&mut self, t: &GameTable, policy: &RolloutPolicy, target: u64) {
        while self.work < target {
            self.iterate(t, policy);
            self.iterations += 1;
            self.work = self.spent();
            crate::mcts::count_rollout();
        }
    }

    /// One iteration: a world, a walk down by UCB1 among the actions available in it to one
    /// not yet played, its leaf's score, and that score for every node on the way.
    fn iterate(&mut self, t: &GameTable, policy: &RolloutPolicy) {
        let mut s = if self.resample {
            let mut rng = self.rng;
            let s = self.deal(t, &mut rng);
            self.rng = rng;
            self.extra += WORLD_STEPS;
            s
        } else {
            self.state.clone()
        };
        self.x.counts.copied();
        self.extra += ITERATION_STEPS;
        let mut path: [u32; 256] = [0; 256];
        let mut depth = 0;
        let mut node = self.root;
        let mut d = self.decision.clone();
        let root_turn = self.state.turn().number();
        let mut plans = [combat::Plan::default(), combat::Plan::default()];
        let value = loop {
            let at_root = node == self.root;
            let keys = if at_root {
                std::mem::take(&mut self.root_keys)
            } else {
                keys(&s, &d, self.known, self.viewer)
            };
            let combat_only = !at_root
                && self.family.options.combat
                && combat::handles(d.kind)
                && s.turn().number() == root_turn;
            let policy_only = !at_root && self.family.options.mid && answered_by_policy(&d);
            let (child, choice) = if combat_only {
                let c = combat::Sim::<_, combat::General>::new(
                    t,
                    policy,
                    &mut self.x,
                    d.player,
                    &mut plans[d.player.index()],
                )
                .choose(&s, &d);
                (self.only(node, keys[usize::from(c.0)], d.player), c)
            } else if policy_only {
                let c = policy.choose(t, Observation::new(&s, d.player), &d);
                (self.only(node, keys[usize::from(c.0)], d.player), c)
            } else {
                self.select(node, &keys, d.player, |x| {
                    policy.scores(t, observe(&s, t, x, d.player), &d)
                })
            };
            if at_root {
                self.root_keys = keys;
            }
            if let Some(trace) = self.trace.as_mut() {
                if at_root {
                    trace.enter(choice);
                } else {
                    trace.advance(t, &s, &d, choice, policy_only || combat_only);
                }
            }
            path[depth] = child;
            depth += 1;
            apply(&mut s, t, &mut self.x, &mut Replay, choice);
            if at_root {
                smart(&mut s);
            }
            if self.nodes[child as usize].visits == 0 || depth == path.len() {
                s.reseed(self.rng.next_u64());
                break self.leaf(&mut s, t, policy);
            }
            match next(&mut s, t, &mut self.x, &mut Replay) {
                Step::Over(outcome) => break value(outcome, self.turns(&s)),
                Step::Decide(below) => {
                    d = below;
                    node = child;
                }
            }
        };
        if let Some(trace) = self.trace.as_mut() {
            trace.score(if self.decision.player == Seat::P0 {
                value
            } else {
                1.0 - value
            });
        }
        for &n in &path[..depth] {
            let n = &mut self.nodes[n as usize];
            n.visits += 1;
            n.reward += if n.mover == Seat::P0 {
                value
            } else {
                1.0 - value
            };
        }
    }

    /// The child of `node` to play next among the actions `keys` names, adding any it lacks
    /// and counting each as available. Try an unplayed action first, by greedy score with
    /// `order` and at random otherwise. Once all have played, use the highest UCB1 bound for
    /// `mover` over its availability, breaking ties at random.
    fn select(
        &mut self,
        node: u32,
        keys: &[u64],
        mover: Seat,
        scores: impl FnOnce(&mut Scratch<L>) -> Vec<Option<i32>>,
    ) -> (u32, Choice) {
        let allowed = self.allowed.filter(|_| node == self.root);
        let pair_keys;
        let keys = if let Some(pair) = allowed {
            pair_keys = pair.map(|i| keys[i]);
            &pair_keys
        } else {
            keys
        };
        let candidate_index = |i| allowed.map_or(i, |pair| pair[i]);
        let mut children = Vec::with_capacity(keys.len());
        for &k in keys {
            children.push(self.only(node, k, mover));
        }
        let unplayed = || {
            children
                .iter()
                .enumerate()
                .filter(|&(_, &c)| self.nodes[c as usize].visits == 0)
        };
        let fresh = unplayed().count() as u64;
        if fresh > 0 {
            let (i, &c) = if self.family.options.order {
                let scores = scores(&mut self.x);
                unplayed().max_by_key(|&(i, _)| {
                    let i = candidate_index(i);
                    (scores[i], std::cmp::Reverse(i))
                })
            } else {
                let pick = self.rng.below(fresh) as usize;
                unplayed().nth(pick)
            }
            .expect("a fresh child");
            return (c, Choice(candidate_index(i) as u16));
        }
        // Equal bounds are broken at random: by index, the first candidate (passing, in a
        // priority decision) would take every tie, and with flat rewards the root's visits
        // would fall away from it candidate by candidate.
        let mut best = (0, f64::NEG_INFINITY);
        let mut equal = 0;
        for (i, &c) in children.iter().enumerate() {
            let n = &self.nodes[c as usize];
            let visits = f64::from(n.visits);
            let bound = n.reward / visits + EXPLORATION * (f64::from(n.avail).ln() / visits).sqrt();
            match bound.total_cmp(&best.1) {
                std::cmp::Ordering::Greater => {
                    best = (i, bound);
                    equal = 1;
                }
                std::cmp::Ordering::Equal => {
                    equal += 1;
                    if self.rng.below(equal) == 0 {
                        best.0 = i;
                    }
                }
                std::cmp::Ordering::Less => {}
            }
        }
        (children[best.0], Choice(candidate_index(best.0) as u16))
    }

    /// The child of `node` for the action `key`, added if it lacks one, counted as available.
    fn only(&mut self, node: u32, key: u64, mover: Seat) -> u32 {
        let c = if let Some(c) = self.child(node, key) {
            c
        } else {
            let c = self.nodes.len() as u32;
            let mut n = Node::new(key, mover);
            n.sibling = self.nodes[node as usize].first;
            self.nodes.push(n);
            self.nodes[node as usize].first = c;
            c
        };
        self.nodes[c as usize].avail += 1;
        c
    }

    /// A new node's score for the first seat, from `s` just after its action.
    fn leaf(&mut self, s: &mut State<L>, t: &GameTable, policy: &RolloutPolicy) -> f64 {
        let mut soft = Softmax {
            rng: &mut self.rng,
            temperature: ROLLOUT_TEMPERATURE,
        };
        match self.family.leaf {
            Leaf::Rollout => {
                let outcome = soft_rollout(s, t, &mut self.x, policy, &mut soft);
                value(outcome, self.turns(s))
            }
            Leaf::Cutoff { turns } => {
                let turn = s.turn().number().saturating_add(u16::from(turns));
                if let Some(outcome) = soft_rollout_to(s, t, &mut self.x, policy, &mut soft, turn) {
                    value(outcome, self.turns(s))
                } else {
                    self.extra += EVAL_STEPS;
                    discounted(evaluate(s, policy.values()), self.turns(s))
                }
            }
            Leaf::Eval => match next(s, t, &mut self.x, &mut Replay) {
                Step::Over(outcome) => value(outcome, self.turns(s)),
                Step::Decide(_) => {
                    self.extra += EVAL_STEPS;
                    discounted(evaluate(s, policy.values()), self.turns(s))
                }
            },
        }
    }

    /// The turns from the tree's first root to `s`.
    fn turns(&self, s: &State<L>) -> u16 {
        s.turn().number().saturating_sub(self.origin)
    }
}

/// Whether an anchored search ([`Options::anchor`]) plays the default answer to
/// decision `d`, seen in `obs`, over its own choice `search` unless that choice leads
/// clearly. Without `guard` this covers a cast where greedy would pass, as Deep Seer once cast
/// Oust on its own token. Not spending a card on a coin flip is nearly free. Elsewhere
/// greedy's answers cost: in run-003's games of the leading family at 10M steps, searched
/// again from fresh seeds, the search's own choice led greedy's answer by about 0.003 at other
/// priority decisions (some 20 a game) and by about 0.002 at targets, objects and the other
/// questions (some 13 a game), so guarding those would cost 0.04 to 0.07 a game.
///
/// Not a card `policy` is told never to cast ([`crate::CardInfo::never_cast`], Mana Crypt):
/// greedy passes on it by tuning for its own play, not judgment, and the search keeps its say.
/// Those were half the switches of the fair `pimc72/hidden/rollout/tuned@30M` (by-bzy).
///
/// With `guard` ([`Options::guard`]), hold against any different attacker declaration
/// ([CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a): "The active player
/// chooses which creatures that they control, if any, will attack"), any choice of up to
/// a number of objects, and picks from the player's own hand except for mulligan bottoms.
/// At priority, hold against a cast or activation scored at 0 or less by the rollout policy,
/// regardless of greedy's answer, with the same `never_cast` exemption.
///
/// With [`Options::combat`], hold the planner's answer at combat declarations and targets.
/// With [`Options::expert`], hold the default at every decision.
pub fn guarded<L: Layout>(
    t: &GameTable,
    policy: &RolloutPolicy,
    obs: Observation<'_, L>,
    d: &Decision<L>,
    default: Choice,
    search: Choice,
    options: Options,
) -> bool {
    if options.expert || (options.combat && combat::handles(d.kind)) {
        return search != default;
    }
    let eligible = |slot: L::Slot| {
        obs.card(slot)
            .is_none_or(|card| !policy.values().get(card).never_cast)
    };
    if !options.guard {
        return d.kind == DecisionKind::Priority
            && d.candidates.option(default) == Some(Opt::Pass)
            && matches!(d.candidates.option(search), Some(Opt::Cast { slot, .. }) if eligible(slot));
    }
    if search == default {
        return false;
    }
    match d.kind {
        DecisionKind::Priority => match d.candidates.option(search) {
            Some(action @ (Opt::Cast { slot, .. } | Opt::Activate { slot, .. })) => {
                eligible(slot) && policy.view(t, obs, d.player, false).action(action) <= 0
            }
            _ => false,
        },
        DecisionKind::Attacker | DecisionKind::ChooseUpTo => true,
        DecisionKind::ChooseObject => {
            d.prompt.purpose != Purpose::PutOnBottom
                && matches!(&d.candidates, Candidates::Objects(slots) if !slots.is_empty())
                && d.candidates.objects().all(|slot| {
                    let object = obs.object(slot);
                    object.zone() == Zone::Hand && object.owner() == d.player
                })
        }
        _ => false,
    }
}

/// Whether a family with [`Options::mid`] lets greedy answer decisions of `kind` below the
/// root. Those asked while casting or activating
/// ([CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012): "To cast a spell is to
/// take it from where it is ... put it on the stack, and pay its costs"), while resolving
/// ([CR 608.2](https://yawgatog.com/resources/magic-rules/#R6082)), and the small
/// bookkeeping choices like them, are: as tree nodes, each child tried once in random order
/// and averaged, they price the option before them as if they were answered at random.
/// Turns, priority and combat stay the tree's: they are the choices it searches for. Every
/// kind is named, so a new one must be decided here. The card a mulligan puts on the bottom
/// stays the tree's too, with the keep it follows.
fn answered_by_policy<L: Layout>(d: &Decision<L>) -> bool {
    match d.kind {
        DecisionKind::Target
        | DecisionKind::TargetCount
        | DecisionKind::X
        | DecisionKind::PaymentPlan
        | DecisionKind::ManaPayment
        | DecisionKind::CastOffer
        | DecisionKind::YesNo
        | DecisionKind::ChooseUpTo
        // Discarding to hand size, the legend to keep and a land's life payment, among the
        // kinds here, and the order of one's own triggers are as wide or as minor, and greedy
        // answers them sensibly.
        | DecisionKind::TriggerOrder => true,
        DecisionKind::ChooseObject => d.prompt.purpose != Purpose::PutOnBottom,
        DecisionKind::Priority
        | DecisionKind::PlayOrDraw
        | DecisionKind::KeepOrMulligan
        | DecisionKind::Attacker
        | DecisionKind::AttackTarget
        | DecisionKind::Blocker
        | DecisionKind::BlockTarget
        | DecisionKind::DamageDivision => false,
    }
}

/// A finished game's score for the first seat, `turns` turns after the tree's first root: a
/// win 1 and a loss 0, discounted toward an even game by [`TURN_DISCOUNT`] each turn; a draw
/// or a stall 0.5.
pub fn value(outcome: Outcome, turns: u16) -> f64 {
    let p0 = match outcome {
        Outcome::Win(Seat::P0) => 1.0,
        Outcome::Win(_) => 0.0,
        Outcome::Draw | Outcome::Stalled(_) => 0.5,
    };
    discounted(p0, turns)
}

/// A score for the first seat, `p0`, discounted toward an even game for `turns` turns: an
/// evaluation as a finished game is, so the two compare on one scale.
fn discounted(p0: f64, turns: u16) -> f64 {
    0.5 + (p0 - 0.5) * TURN_DISCOUNT.powi(i32::from(turns))
}

/// Each candidate's key: its player, the decision's kind and the option with the card it
/// names, if any, told apart as the searcher (`viewer`) tells it apart; then which of the
/// candidates with that much in common it is. A card the searcher knew at the root keys by
/// its slot, the same card in every world. One it knows only in this world (a card it drew)
/// keys by the card, and so does one the option itself shows: a spell cast face up from a
/// hidden hand ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "To propose
/// the casting of a spell, a player first moves that card (or that copy of a card) from
/// where it is to the stack. It becomes the topmost object on the stack. It has all the
/// characteristics of the card"), a land played, a permanent turned face up. Any other card
/// it cannot see (a face-down permanent, a card picked from a library it cannot see) keys by
/// its slot, so the searcher never plans on a card it would not know.
fn keys<L: Layout>(
    world: &State<L>,
    d: &Decision<L>,
    known: Bitboard<L::Slot>,
    viewer: Seat,
) -> Vec<u64> {
    let mut bases = Vec::with_capacity(d.len());
    for i in 0..d.len() {
        let c = Choice(i as u16);
        let mut h = DefaultHasher::new();
        d.player.hash(&mut h);
        d.kind.hash(&mut h);
        let named = |slot: L::Slot, shows: bool| -> (Option<L::Slot>, u8) {
            if !known.contains(slot) && (shows || world.knows(viewer, slot)) {
                (None, world.copiable(slot).key().index() as u8)
            } else {
                (Some(slot), 0)
            }
        };
        match d.candidates {
            Candidates::Objects(_) => {
                0u8.hash(&mut h);
                named(d.candidates.object(c).expect("an object"), false).hash(&mut h);
            }
            Candidates::Options(_) => {
                let opt = d.candidates.option(c).expect("an option");
                1u8.hash(&mut h);
                match names_slot(opt) {
                    Some(slot) => {
                        named(slot, shows(opt)).hash(&mut h);
                        with_slot(opt, L::Slot::from_index(0)).hash(&mut h);
                    }
                    None => opt.hash(&mut h),
                }
            }
        }
        bases.push(h.finish());
    }
    bases
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let ordinal = bases[..i].iter().filter(|&&o| o == b).count() as u64;
            b ^ ordinal.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        })
        .collect()
}

/// Whether taking an option shows everyone the card it names.
fn shows<L: Layout>(opt: Opt<L>) -> bool {
    match opt {
        Opt::Cast { face, .. } => face != FaceIx::DOWN,
        Opt::PlayLand(_) | Opt::TurnFaceUp(_) => true,
        _ => false,
    }
}

/// The slot an option names.
fn names_slot<L: Layout>(opt: Opt<L>) -> Option<L::Slot> {
    match opt {
        Opt::PlayLand(slot)
        | Opt::Attack(slot)
        | Opt::Block(slot)
        | Opt::Lethal(slot)
        | Opt::Object(slot)
        | Opt::TurnFaceUp(slot)
        | Opt::Cast { slot, .. }
        | Opt::Activate { slot, .. }
        | Opt::Mana { slot, .. }
        | Opt::Defender(Some(slot)) => Some(slot),
        _ => None,
    }
}

/// The option with the slot it names replaced.
fn with_slot<L: Layout>(opt: Opt<L>, to: L::Slot) -> Opt<L> {
    match opt {
        Opt::PlayLand(_) => Opt::PlayLand(to),
        Opt::Attack(_) => Opt::Attack(to),
        Opt::Block(_) => Opt::Block(to),
        Opt::Lethal(_) => Opt::Lethal(to),
        Opt::Object(_) => Opt::Object(to),
        Opt::TurnFaceUp(_) => Opt::TurnFaceUp(to),
        Opt::Cast { face, costs, .. } => Opt::Cast {
            slot: to,
            face,
            costs,
        },
        Opt::Activate { ability, .. } => Opt::Activate { slot: to, ability },
        Opt::Mana { color, .. } => Opt::Mana { slot: to, color },
        Opt::Defender(Some(_)) => Opt::Defender(Some(to)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use beryl_cards::CARDS;
    use beryl_cards::decks::WU_CONTROL;
    use beryl_cards::names;
    use beryl_engine::{Profile, Slot, ZoneKey};
    use std::collections::BTreeSet;

    use super::*;

    /// A decision's keys are the same in every world its player could be in, so a tree's
    /// statistics for an action never depend on a card the player cannot see: checked at
    /// every decision of greedy mirror games, among them choices naming face-down permanents
    /// and cards in a library.
    #[test]
    fn keys_are_the_same_in_every_world_the_player_could_be_in() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut unseen = 0;
        for seed in 0..6 {
            let mut s = State::new(&t, seed, Profile::SMART).unwrap();
            let mut x = Scratch::new();
            loop {
                let d = match next(&mut s, &t, &mut x, &mut Replay) {
                    Step::Over(_) => break,
                    Step::Decide(d) => d,
                };
                let obs = observe(&s, &t, &mut x, d.player);
                let known: Bitboard = Slot::all().filter(|&q| obs.knows(q)).collect();
                let world = |i| obs.determinize(&t, &mut SplitMix64::new(i));
                let first = keys(&world(0), &d, known, d.player);
                for i in 1..4 {
                    assert_eq!(keys(&world(i), &d, known, d.player), first, "seed {seed}");
                }
                unseen += (0..d.len())
                    .filter_map(|i| match d.candidates {
                        Candidates::Objects(_) => d.candidates.object(Choice(i as u16)),
                        Candidates::Options(_) => {
                            names_slot(d.candidates.option(Choice(i as u16))?)
                        }
                    })
                    .filter(|&q| !known.contains(q))
                    .count();
                let c = policy.choose(&t, obs, &d);
                apply(&mut s, &t, &mut x, &mut Replay, c);
            }
        }
        assert!(
            unseen > 0,
            "no choice named a card its player could not see"
        );
    }

    /// The candidate visited most leads; of equals, the better mean, not the first.
    #[test]
    fn equal_visits_go_to_the_better_mean() {
        assert_eq!(leader(&[5, 9, 3], &[4.0, 1.0, 3.0]), 1);
        assert_eq!(leader(&[6, 6, 2], &[1.0, 4.0, 2.0]), 1);
        assert_eq!(leader(&[6, 6], &[3.0, 3.0]), 0);
        assert_eq!(leader(&[0, 0], &[0.0, 0.0]), 0);
    }

    /// Root stats for [`by_means`] from each tree's `(visits, reward)` per candidate.
    fn stats<const N: usize>(trees: &[[(u32, f64); N]]) -> Vec<Vec<Option<(u32, f64)>>> {
        trees
            .iter()
            .map(|t| t.iter().map(|&c| Some(c)).collect())
            .collect()
    }

    /// Where the vote and the tree-averaged means disagree by more than the noise across trees,
    /// the means choose.
    #[test]
    fn means_overrule_the_vote_past_the_noise() {
        // Candidate 0 has the most visits; 1 leads it by about 0.1 in every tree.
        let trees = stats(&[
            [(100, 50.0), (60, 36.0)],
            [(100, 52.0), (60, 37.2)],
            [(100, 48.0), (60, 34.8)],
            [(100, 51.0), (60, 36.6)],
        ]);
        assert_eq!(leader(&[400, 240], &[201.0, 144.6]), 0);
        assert_eq!(by_means(&trees), 1);
    }

    /// Where the vote and the means agree, or the lead is within the noise, the vote stands.
    #[test]
    fn the_vote_stands_where_the_means_agree_or_are_noise() {
        let agree = stats(&[[(100, 60.0), (60, 24.0)], [(100, 62.0), (60, 25.0)]]);
        assert_eq!(by_means(&agree), 0);
        // Candidate 1 leads on average, but one tree says the opposite by as much.
        let noisy = stats(&[
            [(100, 50.0), (60, 42.0)],
            [(100, 50.0), (60, 24.0)],
            [(100, 50.0), (60, 33.0)],
        ]);
        assert_eq!(by_means(&noisy), 0);
        // A lead measured by a single tree has no noise to clear, so it never overrules.
        assert_eq!(by_means(&stats(&[[(100, 50.0), (60, 50.0)]])), 0);
        // Equal visits go to the better mean, as `leader` has it.
        assert_eq!(
            by_means(&stats(&[[(10, 2.0), (10, 8.0)], [(10, 2.0), (10, 8.0)]])),
            1
        );
    }

    /// A candidate with fewer than a tenth of the leader's visits is never chosen, however
    /// good its mean.
    #[test]
    fn means_never_choose_below_the_visit_floor() {
        let trees = stats(&[
            [(100, 50.0), (4, 4.0), (20, 11.0)],
            [(100, 50.0), (5, 5.0), (20, 11.2)],
            [(100, 50.0), (0, 0.0), (20, 10.8)],
        ]);
        // 1 has the best mean on every tree that tried it, but 9 visits of 300; 2 has 60.
        assert_eq!(by_means(&trees), 2);
        let starved = stats(&[[(100, 50.0), (4, 4.0)], [(100, 50.0), (5, 5.0)]]);
        assert_eq!(by_means(&starved), 0);
    }

    /// A family reads back as it prints.
    #[test]
    fn families_round_trip() {
        for s in [
            "pimc18/hidden/rollout/tuned",
            "ismcts18/hands/cut2/estimate",
            "ismcts4/omniscient/eval/estimate",
            "pimc18/omniscient/rollout/tuned/anchor",
            "pimc18/omniscient/rollout/tuned/mid",
            "pimc72/hidden/rollout/tuned/anchor+mid",
            "pimc72/hidden/rollout/tuned/anchor+means",
            "pimc72/hidden/rollout/tuned/means",
            "pimc72/hidden/rollout/tuned/anchor+mid+means",
            "pimc72/hidden/rollout/tuned/anchor+guard",
            "pimc72/hidden/rollout/tuned/anchor+guard+mid+means",
            "pimc72/hidden/rollout/tuned/anchor+hold",
            "pimc72/hidden/rollout/tuned/anchor+guard+hold+waste+combat",
            "pimc72/hidden/rollout/tuned/anchor+guard+mid+means+hold+waste+combat",
            "pimc72/hidden/rollout/tuned/anchor+expert",
            "pimc72/hidden/rollout/tuned/anchor+order",
            "pimc72/hidden/rollout/tuned/anchor+expert+guard+mid+means+hold+waste+combat+order",
        ] {
            let f: Family = s.parse().unwrap();
            assert_eq!(f.to_string(), s);
        }
        let plain: Family = "pimc18/hidden/rollout/tuned".parse().unwrap();
        assert_eq!(plain.options, Options::default());
        for bad in [
            "pimc0/hidden/rollout/tuned",
            "mcts/hidden/rollout/tuned",
            "pimc1/x/y",
            "pimc18/hidden/rollout/tuned/",
            "pimc18/hidden/rollout/tuned/mid+anchor",
            "pimc18/hidden/rollout/tuned/anchor+anchor",
            "pimc18/hidden/rollout/tuned/deep",
            "pimc18/hidden/rollout/tuned/anchor/mid",
            "pimc18/hidden/rollout/tuned/means+anchor",
            "pimc18/hidden/rollout/tuned/means+means",
            "pimc18/hidden/rollout/tuned/guard",
            "pimc18/hidden/rollout/tuned/guard+mid",
            "pimc18/hidden/rollout/tuned/guard+anchor",
            "pimc18/hidden/rollout/tuned/hold+anchor",
            "pimc18/hidden/rollout/tuned/expert",
            "pimc18/hidden/rollout/tuned/anchor+guard+expert",
            "pimc18/hidden/rollout/tuned/anchor+order+combat",
        ] {
            assert!(bad.parse::<Family>().is_err(), "{bad}");
        }
    }

    /// The anchor yields to the search's choice only for a lead past both the margin and the
    /// noise across the trees.
    #[test]
    fn the_anchor_yields_only_to_a_clear_lead() {
        let check = |lead, error, trees| AnchorCheck {
            greedy: Choice(0),
            search: Choice(1),
            lead,
            error,
            trees,
        };
        // The Deep Seer incident's Oust on its own token led passing by 0.006.
        assert!(!check(0.006, 0.001, 18).overrides());
        assert!(check(0.05, 0.01, 18).overrides());
        assert!(!check(0.05, 0.03, 18).overrides(), "within the noise");
        assert!(
            check(0.03, 0.5, 1).overrides(),
            "one tree: the margin alone"
        );
        assert!(!check(0.5, 0.0, 0).overrides(), "no tree visited both");
        assert_eq!(check(0.05, 0.01, 18).played(), Choice(1));
        assert_eq!(check(0.006, 0.001, 18).played(), Choice(0));
        let agreed = AnchorCheck {
            search: Choice(0),
            ..check(0.5, 0.0, 18)
        };
        assert_eq!(agreed.played(), Choice(0));
        let (mean, error) = mean_and_error(&[0.1, 0.3]);
        assert!((mean - 0.2).abs() < 1e-12 && (error - 0.1).abs() < 1e-12);
        assert_eq!(mean_and_error(&[0.4]), (0.4, 0.0));
        assert_eq!(mean_and_error(&[]), (0.0, 0.0));
    }

    /// A family of one omniscient world with `options`, for tests.
    fn family(options: Options) -> Family {
        Family {
            method: Method::Pimc { worlds: 1 },
            sight: Sight::Omniscient,
            leaf: Leaf::Rollout,
            knowledge: Knowledge::Tuned,
            options,
        }
    }

    /// The priority decisions of a greedy mirror game from `seed` that offer a spell or an
    /// ability, each with the state it was asked in: where a search's root actions lead to
    /// questions asked while casting or resolving.
    fn positions(t: &GameTable, seed: u64) -> Vec<(State, Decision)> {
        let policy = RolloutPolicy::new(t);
        let mut s = State::new(t, seed, Profile::SMART).unwrap();
        let mut x = Scratch::new();
        let mut out = Vec::new();
        loop {
            let d = match next(&mut s, t, &mut x, &mut Replay) {
                Step::Over(_) => break,
                Step::Decide(d) => d,
            };
            let acts = (0..d.len()).any(|i| {
                matches!(
                    d.candidates.option(Choice(i as u16)),
                    Some(Opt::Cast { .. } | Opt::Activate { .. })
                )
            });
            if d.kind == DecisionKind::Priority && acts {
                out.push((s, d));
            }
            let c = policy.choose(t, observe(&s, t, &mut x, d.player), &d);
            apply(&mut s, t, &mut x, &mut Replay, c);
        }
        out
    }

    #[test]
    fn order_expands_the_greedy_root_choice_first() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut x = Scratch::new();
        let (s, d, greedy) = positions(&t, 1)
            .into_iter()
            .find_map(|(s, d)| {
                let greedy = policy.choose(&t, observe(&s, &t, &mut x, d.player), &d);
                (greedy != Choice(0)).then_some((s, d, greedy))
            })
            .expect("a priority decision where greedy plays an action");
        let obs = observe(&s, &t, &mut x, d.player);
        let known = Slot::all().filter(|&slot| obs.knows(slot)).collect();
        let ordered = family(Options {
            order: true,
            ..Options::default()
        });
        let greedy = usize::from(greedy.0);
        for seed in 0..4 {
            for allowed in [None, Some([greedy, 0])] {
                let mut tree = Tree::new(&t, ordered, obs, &d, known, false, seed);
                tree.allowed = allowed;
                tree.iterate(&t, &policy);
                for (i, child) in tree.root_children().into_iter().enumerate() {
                    let visits = child.map_or(0, |c| tree.nodes[c as usize].visits);
                    assert_eq!(visits, u32::from(i == greedy));
                }
            }
        }
    }

    /// With `mid`, a question asked below the root while casting or resolving has one child in
    /// a fixed world, greedy's answer, while the root still tries every candidate.
    #[test]
    fn mid_lets_greedy_answer_below_the_root() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut x = Scratch::new();
        let mid = family(Options {
            mid: true,
            ..Options::default()
        });
        let mut answered = 0;
        for (s, d) in positions(&t, 1).into_iter().take(10) {
            let obs = observe(&s, &t, &mut x, d.player);
            let known: Bitboard = Slot::all().filter(|&q| obs.knows(q)).collect();
            let mut tree = Tree::new(&t, mid, obs, &d, known, false, 7);
            tree.grow_to(&t, &policy, 10_000);
            assert_eq!(tree.root_children().iter().flatten().count(), d.len());
            answered += walk(&t, &tree, tree.root, &tree.state, &d, 0);
        }
        assert!(answered > 0, "no question below the root was met");
    }

    /// Walks `tree` a few levels below `node`, reached in `s` at decision `d`: every node at a
    /// question greedy answers has at most one child. Returns how many it met.
    fn walk(
        t: &GameTable,
        tree: &Tree<Small>,
        node: u32,
        s: &State,
        d: &Decision,
        depth: u8,
    ) -> u32 {
        if depth == 4 {
            return 0;
        }
        let keys = keys(s, d, tree.known, tree.viewer);
        let mut met = 0;
        let mut c = tree.nodes[node as usize].first;
        while c != NONE {
            let n = tree.nodes[c as usize];
            let i = keys
                .iter()
                .position(|&k| k == n.key)
                .expect("a child's action");
            let (mut s, mut x) = (*s, Scratch::new());
            apply(&mut s, t, &mut x, &mut Replay, Choice(i as u16));
            if node == tree.root {
                smart(&mut s);
            }
            if let Step::Decide(below) = next(&mut s, t, &mut x, &mut Replay) {
                if n.first != NONE && answered_by_policy(&below) {
                    let mut children = 0;
                    let mut g = n.first;
                    while g != NONE {
                        children += 1;
                        g = tree.nodes[g as usize].sibling;
                    }
                    assert_eq!(children, 1, "{:?} below the root", below.kind);
                    met += 1;
                }
                met += walk(t, tree, c, &s, &below, depth + 1);
            }
            c = n.sibling;
        }
        met
    }

    /// An anchored search plays greedy's answer where it guards it, unless its own choice
    /// leads clearly, and records its check; elsewhere it plays as the plain search does.
    #[test]
    fn an_anchored_search_plays_greedy_unless_clearly_beaten() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut x = Scratch::new();
        let settings = |options| Settings {
            family: Family {
                method: Method::Pimc { worlds: 2 },
                ..family(options)
            },
            work: 8_000,
            threads: 1,
        };
        let anchor = Options {
            anchor: true,
            ..Options::default()
        };
        let (mut searched, mut checked) = (0, 0);
        // Where the guard can act: greedy passes, and a spell it would hold is on offer.
        let holds = |s: &State, d: &Decision, x: &mut Scratch| {
            let obs = observe(s, &t, x, d.player);
            let g = policy.choose(&t, obs, d);
            (0..d.len()).any(|i| guarded(&t, &policy, obs, d, g, Choice(i as u16), anchor))
        };
        let mut scratch = Scratch::new();
        let chosen: Vec<(State, Decision)> = (2..12)
            .flat_map(|seed| positions(&t, seed))
            .filter(|(s, d)| holds(s, d, &mut scratch))
            .take(16)
            .collect();
        for (i, (s, d)) in chosen.into_iter().enumerate() {
            let obs = observe(&s, &t, &mut x, d.player);
            let mut anchored = LongSearch::<Small>::new(&t, settings(anchor), i as u64);
            let mut plain = LongSearch::<Small>::new(&t, settings(Options::default()), i as u64);
            let c = anchored.choose(obs, &d, AgentDecision(0));
            let free = plain.choose(obs, &d, AgentDecision(0));
            assert!(plain.last_search().is_none_or(|s| s.anchor.is_none()));
            let Some(search) = anchored.last_search() else {
                continue;
            };
            searched += 1;
            let greedy = policy.choose(&t, obs, &d);
            if let Some(a) = search.anchor {
                assert_eq!(a.greedy, greedy);
                assert!(guarded(&t, &policy, obs, &d, greedy, a.search, anchor));
                assert_eq!(c, a.played());
                if c != greedy {
                    assert!(a.lead > ANCHOR_MARGIN && a.lead > ANCHOR_ERRORS * a.error);
                }
                checked += 1;
            } else {
                assert!(c == greedy || !guarded(&t, &policy, obs, &d, greedy, c, anchor));
                assert_eq!(c, free, "unguarded, it plays as the plain search");
            }
        }
        assert!(searched > 3, "{searched} searched");
        assert!(checked > 0, "no guarded decision met");
    }

    /// A win scores more the sooner it comes and a loss more the later, a draw even, and an
    /// evaluation on the same scale.
    #[test]
    fn sooner_wins_and_later_losses_score_higher() {
        let (win, loss) = (Outcome::Win(Seat::P0), Outcome::Win(Seat::P1));
        assert!((value(win, 0) - 1.0).abs() < 1e-12 && value(loss, 0).abs() < 1e-12);
        assert!(value(win, 3) > value(win, 8));
        assert!(value(loss, 8) > value(loss, 3));
        assert!(value(win, 40) > 0.5 && value(loss, 40) < 0.5);
        assert!((value(Outcome::Draw, 7) - 0.5).abs() < 1e-12);
        assert!(
            (value(win, 5) + value(loss, 5) - 1.0).abs() < 1e-12,
            "zero-sum"
        );
        assert!((discounted(0.8, 5) - (0.5 + 0.3 * TURN_DISCOUNT.powi(5))).abs() < 1e-12);
        assert!(discounted(0.8, 5) < value(win, 5));
    }

    /// A spell greedy is told never to cast (Mana Crypt) is not guarded: greedy passes on it
    /// by tuning, not judgment. Any other spell greedy passes on is.
    #[test]
    fn the_anchor_does_not_hold_what_greedy_never_casts() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut x = Scratch::new();
        let (mut never, mut other) = (0, 0);
        for seed in 0..40 {
            let mut s = State::new(&t, seed, Profile::SMART).unwrap();
            loop {
                let d = match next(&mut s, &t, &mut x, &mut Replay) {
                    Step::Over(_) => break,
                    Step::Decide(d) => d,
                };
                let obs = observe(&s, &t, &mut x, d.player);
                let g = policy.choose(&t, obs, &d);
                if d.kind == DecisionKind::Priority && d.candidates.option(g) == Some(Opt::Pass) {
                    for i in 0..d.len() {
                        let c = Choice(i as u16);
                        if let Some(Opt::Cast { slot, .. }) = d.candidates.option(c) {
                            let crypt = obs
                                .card(slot)
                                .is_some_and(|card| t.def(card).name == names::MANA_CRYPT);
                            assert_eq!(
                                guarded(&t, &policy, obs, &d, g, c, Options::default()),
                                !crypt
                            );
                            if crypt {
                                let options = Options {
                                    guard: true,
                                    ..Options::default()
                                };
                                assert!(!guarded(&t, &policy, obs, &d, g, c, options));
                                never += 1;
                            } else {
                                other += 1;
                            }
                        }
                    }
                }
                apply(&mut s, &t, &mut x, &mut Replay, g);
            }
            if never > 0 && other > 0 {
                break;
            }
        }
        assert!(never > 0 && other > 0, "Mana Crypt {never}, others {other}");
    }
    /// The decisions of a greedy mirror game from `seed`, each with the state it was asked in
    /// and greedy's answer.
    fn greedy_decisions(t: &GameTable, seed: u64) -> Vec<(State, Decision, Choice)> {
        let policy = RolloutPolicy::new(t);
        let mut s = State::new(t, seed, Profile::SMART).unwrap();
        let mut x = Scratch::new();
        let mut out = Vec::new();
        loop {
            let d = match next(&mut s, t, &mut x, &mut Replay) {
                Step::Over(_) => break,
                Step::Decide(d) => d,
            };
            let g = policy.choose(t, observe(&s, t, &mut x, d.player), &d);
            out.push((s, d, g));
            apply(&mut s, t, &mut x, &mut Replay, g);
        }
        out
    }

    /// Cover each guarded choice and nearby exceptions with decisions from greedy mirror games.
    #[test]
    fn the_guard_holds_at_attackers_optional_picks_hand_costs_and_pass_scored_plays() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut x = Scratch::new();
        let options = Options {
            anchor: true,
            guard: true,
            ..Options::default()
        };
        let mut remaining = BTreeSet::from([
            "attacker",
            "attack target",
            "find",
            "other optional pick",
            "hand pick",
            "mulligan bottom",
            "other object",
            "cast scored as a pass",
            "activation scored as a pass",
            "positive play",
        ]);
        for seed in 0..60 {
            for (s, d, g) in greedy_decisions(&t, seed) {
                let obs = observe(&s, &t, &mut x, d.player);
                let view = policy.view(&t, obs, d.player, false);
                let hand = obs.zone(ZoneKey::Of(d.player, Zone::Hand));
                assert!(!guarded(&t, &policy, obs, &d, g, g, options));
                for i in 0..d.len() {
                    let c = Choice(i as u16);
                    if c == g {
                        continue;
                    }
                    let (case, expected) = match d.kind {
                        DecisionKind::Attacker => ("attacker", true),
                        DecisionKind::AttackTarget => ("attack target", false),
                        DecisionKind::ChooseUpTo if d.prompt.purpose == Purpose::Search => {
                            ("find", true)
                        }
                        DecisionKind::ChooseUpTo => ("other optional pick", true),
                        DecisionKind::ChooseObject if d.prompt.purpose == Purpose::PutOnBottom => {
                            ("mulligan bottom", false)
                        }
                        DecisionKind::ChooseObject
                            if d.candidates.objects().all(|slot| hand.contains(slot)) =>
                        {
                            ("hand pick", true)
                        }
                        DecisionKind::ChooseObject => ("other object", false),
                        DecisionKind::Priority => match d.candidates.option(c) {
                            Some(Opt::Cast { slot, .. })
                                if obs
                                    .card(slot)
                                    .is_some_and(|card| policy.values().get(card).never_cast) =>
                            {
                                ("never cast", false)
                            }
                            Some(action @ Opt::Cast { .. }) if view.action(action) <= 0 => {
                                if d.candidates.option(g) == Some(Opt::Pass) {
                                    ("cast against pass", true)
                                } else {
                                    ("cast scored as a pass", true)
                                }
                            }
                            Some(action @ Opt::Activate { .. }) if view.action(action) <= 0 => {
                                ("activation scored as a pass", true)
                            }
                            Some(Opt::Cast { .. } | Opt::Activate { .. }) => {
                                ("positive play", false)
                            }
                            _ => ("other priority action", false),
                        },
                        _ => ("other decision", false),
                    };
                    assert_eq!(
                        guarded(&t, &policy, obs, &d, g, c, options),
                        expected,
                        "{case}"
                    );
                    remaining.remove(case);
                }
            }
            if remaining.is_empty() {
                return;
            }
        }
        panic!("missing decisions: {remaining:?}");
    }

    /// An `anchor+guard` search plays greedy's attacker unless its own choice leads clearly.
    #[test]
    fn a_guarded_search_plays_greedy_at_attackers_unless_clearly_beaten() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let policy = RolloutPolicy::new(&t);
        let mut x = Scratch::new();
        let settings = Settings {
            family: Family {
                method: Method::Pimc { worlds: 2 },
                ..family(Options {
                    anchor: true,
                    guard: true,
                    ..Options::default()
                })
            },
            work: 8_000,
            threads: 1,
        };
        let chosen: Vec<(State, Decision, Choice)> = (0..12)
            .flat_map(|seed| greedy_decisions(&t, seed))
            .filter(|(_, d, _)| d.kind == DecisionKind::Attacker && d.len() > 1)
            .take(12)
            .collect();
        let mut checked = 0;
        for (i, (s, d, greedy)) in chosen.into_iter().enumerate() {
            let obs = observe(&s, &t, &mut x, d.player);
            let mut guarded_search = LongSearch::<Small>::new(&t, settings, i as u64);
            let c = guarded_search.choose(obs, &d, AgentDecision(0));
            let search = guarded_search.last_search().expect("searched");
            match search.anchor {
                Some(a) => {
                    assert_eq!(a.greedy, greedy);
                    assert_ne!(a.search, greedy);
                    assert_eq!(c, a.played());
                    if c != greedy {
                        assert!(a.lead > ANCHOR_MARGIN && a.lead > ANCHOR_ERRORS * a.error);
                    }
                    checked += 1;
                }
                None => assert_eq!(c, greedy, "unchecked, the search agreed with greedy"),
            }
            assert!(
                c == greedy || guarded(&t, &policy, obs, &d, greedy, c, settings.family.options)
            );
        }
        assert!(checked > 0, "no attacker decision checked");
    }

    #[test]
    fn an_expert_anchored_search_checks_nonpriority_choices() {
        let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
        let settings = Settings {
            family: "pimc2/hidden/rollout/tuned/anchor+expert".parse().unwrap(),
            work: 8_000,
            threads: 1,
        };
        let mut x = Scratch::new();
        let (mut held, mut overruled) = (0, 0);
        for (s, d, greedy) in (0..12).flat_map(|seed| greedy_decisions(&t, seed)) {
            if d.len() < 2 || matches!(d.kind, DecisionKind::Priority | DecisionKind::Attacker) {
                continue;
            }
            let obs = observe(&s, &t, &mut x, d.player);
            let expert = ExpertAgent::new(&t, 5).choose(obs, &d, AgentDecision(0));
            if expert == greedy {
                continue;
            }
            let mut anchored = LongSearch::new(&t, settings, 5);
            let choice = anchored.choose(obs, &d, AgentDecision(0));
            if let Some(check) = anchored.last_search().expect("searched").anchor {
                assert_eq!(check.greedy, expert);
                assert_ne!(check.search, expert);
                if choice == expert {
                    assert!(!check.overrides());
                    held += 1;
                } else {
                    assert_eq!(choice, check.search);
                    assert!(check.overrides());
                    overruled += 1;
                }
            } else {
                assert_eq!(choice, expert);
            }
            if held > 0 && overruled > 0 {
                break;
            }
        }
        assert!(held > 0, "no disagreement held the expert's answer");
        assert!(
            overruled > 0,
            "no disagreement overruled the expert's answer"
        );
    }
}
