//! The agents (architecture §19.1): everything that plays the game, behind one trait. The
//! seeded [`RandomAgent`] fuzzes; the [`GreedyAgent`] is the [`RolloutPolicy`] search plays
//! rollouts with; [`Mcts`] searches with it; the [`ExpertAgent`] plays by hand-written rules,
//! and the [`FrozenExpertAgent`] is a copy of it that never changes, the bar general agents
//! are measured against. [`AGENTS`] names every agent a game can be played against.

mod eval;
mod expert;
mod frozen;
mod long;
mod mcts;
mod rollout;
mod values;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Decision, GameTable, Observation};
use beryl_engine::{Large, Layout, Small};
use serde::{Deserialize, Serialize};

pub use eval::{FEATURES, Features, NAMES, WEIGHTS, evaluate, features, logit, sigmoid};
pub use expert::ExpertAgent;
pub use frozen::FrozenExpertAgent;
pub use long::{
    ANCHOR_ERRORS, ANCHOR_MARGIN, AnchorCheck, EVAL_STEPS, Family, Leaf, LongSearch, Method,
    Options, Settings, TURN_DISCOUNT, WORLD_STEPS, guarded,
};
pub use mcts::{Budget, Mcts, Search, Sight, rollouts_run};
pub use rollout::{
    COUNTER_MIN, Inline, REMOVAL_MIN, RolloutPolicy, Softmax, rollout, rollout_to, soft_rollout,
    soft_rollout_to,
};
pub use values::{CardInfo, CardValues, Knowledge};

/// A decision's index among one agent's own, from 0, unsearched ones included: not its index
/// in the game, which counts both players' decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentDecision(pub u32);

impl AgentDecision {
    /// The decision after this one.
    #[must_use]
    pub fn next(self) -> Self {
        AgentDecision(self.0 + 1)
    }
}

/// A player: answers decisions from what its seat can see.
pub trait Agent<L: Layout = Small> {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Choice;

    /// The search behind the last answer; none if it was answered without one, or by an
    /// agent that does not search.
    fn last_search(&self) -> Option<&Search> {
        None
    }

    /// For an agent a game resumed midway must rebuild ([`Mcts::resumed`]), the decision the
    /// rebuilt agent must be asked first to play on as this one; none for an agent whose
    /// state follows the game when it is asked every decision again.
    fn resume_point(&self) -> Option<AgentDecision> {
        None
    }

    /// Its next decision's index among its own, to go back to with [`Agent::rewind`]; none
    /// for an agent that cannot go back, which plays on from where it is.
    fn next_decision(&self) -> Option<AgentDecision> {
        None
    }

    /// Goes back to stand before its own decision `to`, which [`Agent::next_decision`] named,
    /// as a game taken back to there does: it forgets what it kept from the decisions since,
    /// and searches each again from the same seed it had.
    fn rewind(&mut self, _to: AgentDecision) {}
}

/// A boxed agent, as the registry builds them, plays like the agent inside.
impl<L: Layout, T: Agent<L> + ?Sized> Agent<L> for Box<T> {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Choice {
        (**self).choose(obs, d)
    }

    fn last_search(&self) -> Option<&Search> {
        (**self).last_search()
    }

    fn resume_point(&self) -> Option<AgentDecision> {
        (**self).resume_point()
    }

    fn next_decision(&self) -> Option<AgentDecision> {
        (**self).next_decision()
    }

    fn rewind(&mut self, to: AgentDecision) {
        (**self).rewind(to);
    }
}

/// An agent for a player game, which plays in [`Small`] and, should the game outgrow it, goes
/// on in [`Large`]: an agent of both layouts.
pub trait GameAgent: Agent<Small> + Agent<Large> {}

impl<T: Agent<Small> + Agent<Large> + ?Sized> GameAgent for T {}

/// A [`Small`] agent that can play on in [`Large`] as it would have in `Small`: the same
/// streams, and the same place among its own decisions.
pub trait Widen: Agent<Small> {
    type Large: Agent<Large>;

    fn widen(&self) -> Self::Large;
}

/// A [`GameAgent`] made of a [`Small`] agent: once its game is widened into [`Large`], it
/// plays there as that agent widened ([`Widen`]), made the first time it is asked.
pub struct Widening<A: Widen> {
    small: A,
    large: Option<A::Large>,
}

impl<A: Widen> Widening<A> {
    pub fn new(small: A) -> Self {
        Self { small, large: None }
    }
}

impl<A: Widen> Agent<Small> for Widening<A> {
    fn choose(&mut self, obs: Observation<'_, Small>, d: &Decision) -> Choice {
        self.small.choose(obs, d)
    }

    fn last_search(&self) -> Option<&Search> {
        self.small.last_search()
    }

    fn resume_point(&self) -> Option<AgentDecision> {
        self.small.resume_point()
    }

    fn next_decision(&self) -> Option<AgentDecision> {
        self.small.next_decision()
    }

    fn rewind(&mut self, to: AgentDecision) {
        self.small.rewind(to);
    }
}

impl<A: Widen> Agent<Large> for Widening<A> {
    fn choose(&mut self, obs: Observation<'_, Large>, d: &Decision<Large>) -> Choice {
        let small = &self.small;
        self.large
            .get_or_insert_with(|| small.widen())
            .choose(obs, d)
    }

    fn last_search(&self) -> Option<&Search> {
        self.large.as_ref().and_then(Agent::last_search)
    }

    fn resume_point(&self) -> Option<AgentDecision> {
        match &self.large {
            Some(large) => large.resume_point(),
            None => self.small.resume_point(),
        }
    }

    fn next_decision(&self) -> Option<AgentDecision> {
        match &self.large {
            Some(large) => large.next_decision(),
            None => self.small.next_decision(),
        }
    }

    /// A game taken back to before it outgrew [`Small`] goes on in [`Large`] all the same, so
    /// the agent widened rewinds, if there is one yet; else the small one, which it widens from.
    fn rewind(&mut self, to: AgentDecision) {
        match &mut self.large {
            Some(large) => large.rewind(to),
            None => self.small.rewind(to),
        }
    }
}

/// Picks uniformly among the candidates, from its own seeded stream, so a seed reproduces its
/// games.
#[derive(Clone, Debug)]
pub struct RandomAgent {
    rng: SplitMix64,
}

impl RandomAgent {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: SplitMix64::new(seed),
        }
    }
}

impl<L: Layout> Agent<L> for RandomAgent {
    fn choose(&mut self, _obs: Observation<'_, L>, d: &Decision<L>) -> Choice {
        Choice(self.rng.below(d.len() as u64) as u16)
    }
}

/// The rollout policy as an opponent (decision 4 of the Milestone 1 plan): deterministic, so
/// it needs no seed. It keeps its own copy of the game's table, since an agent outlives the
/// borrow it was built from.
#[derive(Clone, Debug)]
pub struct GreedyAgent {
    table: GameTable,
    policy: RolloutPolicy,
}

impl GreedyAgent {
    pub fn new(t: &GameTable) -> Self {
        Self {
            table: t.clone(),
            policy: RolloutPolicy::new(t),
        }
    }
}

impl<L: Layout> Agent<L> for GreedyAgent {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Choice {
        self.policy.choose(&self.table, obs, d)
    }
}

/// An agent a game can be played against, by name: the registry the app and the CLI choose
/// opponents from.
#[derive(Clone, Copy, Debug)]
pub struct AgentInfo {
    pub name: &'static str,
    /// What a human calls it.
    pub title: &'static str,
    /// One line for a human choosing an opponent.
    pub description: &'static str,
    /// Builds the agent for a game from a seed, so a seed reproduces its games, and the
    /// budget a searching agent may spend on a decision; the others ignore it.
    pub new: fn(t: &GameTable, seed: u64, budget: Budget) -> Box<dyn GameAgent + Send>,
}

/// Every agent, in the order a human is offered them.
pub static AGENTS: &[AgentInfo] = &[
    AgentInfo {
        name: "random",
        title: "Random",
        description: "Chooses uniformly at random among its legal options",
        new: random,
    },
    AgentInfo {
        name: "greedy",
        title: "Greedy",
        description: "Plays its most valuable card, attacks when safe, never looks ahead",
        new: greedy,
    },
    AgentInfo {
        name: "expert",
        title: "Expert",
        description: "Plays by a strong player's rules for this deck, and works out combat",
        new: expert,
    },
    AgentInfo {
        name: "expert-frozen",
        title: "Frozen Expert",
        description: "The expert as it stood when retuned for this deck, kept as a fixed bar",
        new: expert_frozen,
    },
    AgentInfo {
        name: "mcts-omniscient",
        title: "Seer",
        description: "Cheats: searches the true game, your hand and its library order included",
        new: mcts_omniscient,
    },
    AgentInfo {
        name: "mcts-hidden",
        title: "Searcher",
        description: "Searches many guesses at the cards it cannot see, playing each out greedily",
        new: mcts_hidden,
    },
    AgentInfo {
        name: "pimc18-omniscient-30m-anchor",
        title: "Deep Seer",
        description: "Cheats as the Seer does, thinking seconds a move",
        new: deep_seer,
    },
    AgentInfo {
        name: "pimc72-hidden-30m-anchor",
        title: "Deep Searcher",
        description: "Guesses at the cards it cannot see, thinking seconds a move",
        new: deep_searcher,
    },
];

/// Agents no longer offered, kept buildable by name so that games recorded against them can
/// still be reviewed. A review re-searches with today's search, so where the search has
/// changed since (`TURN_DISCOUNT`, for one) it may not choose as the game did.
pub static RETIRED: &[AgentInfo] = &[AgentInfo {
    name: "pimc18-omniscient-30m",
    title: "Deep Seer, unanchored",
    description: "Deep Seer before it held spells it was unsure of",
    new: deep_seer_unanchored,
}];

fn random(_t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn GameAgent + Send> {
    Box::new(RandomAgent::new(seed))
}

fn greedy(t: &GameTable, _seed: u64, _budget: Budget) -> Box<dyn GameAgent + Send> {
    Box::new(GreedyAgent::new(t))
}

fn expert(t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn GameAgent + Send> {
    Box::new(Widening::new(ExpertAgent::<Small>::new(t, seed)))
}

fn expert_frozen(t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn GameAgent + Send> {
    Box::new(Widening::new(FrozenExpertAgent::new(t, seed)))
}

fn mcts_omniscient(t: &GameTable, seed: u64, budget: Budget) -> Box<dyn GameAgent + Send> {
    Box::new(Widening::new(Mcts::<Small>::new(
        t,
        Sight::Omniscient,
        budget,
        seed,
    )))
}

fn mcts_hidden(t: &GameTable, seed: u64, budget: Budget) -> Box<dyn GameAgent + Send> {
    Box::new(Widening::new(Mcts::<Small>::new(
        t,
        Sight::Hidden,
        budget,
        seed,
    )))
}

/// `pimc18/omniscient/rollout/tuned/anchor@30M`: the long-think arena's leading family in
/// run-003, anchored (epic by-7wl) so that it casts no spell greedy would hold unless its
/// search is sure, as the unanchored one cast Oust on its own token. Re-searching that game,
/// the anchor changed that move alone. `mid` is left to the arena: there it changed 16 of the
/// game's 62 searched moves and did not lift Jace's 0. 30M engine steps a decision whatever
/// `budget` says, on every core. Its moves do not depend on the threads.
fn deep_seer(t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn GameAgent + Send> {
    deep(
        t,
        seed,
        Method::Pimc { worlds: 18 },
        Sight::Omniscient,
        ANCHORED,
    )
}

/// `pimc72/hidden/rollout/tuned/anchor@30M`: Deep Seer without the cheat, from the long-think
/// arena's strongest fair family in run-003 (72 worlds dealt from what it can see), anchored
/// as Deep Seer is. 30M engine steps a decision whatever `budget` says, on every core.
///
/// The ladder's champion (docs/ai-strategy.md §3): always the current best fair agent, so a
/// promotion changes [`DEEP_SEARCHER`], and its comment names the match that made it.
fn deep_searcher(t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn GameAgent + Send> {
    deep_family(t, seed, DEEP_SEARCHER)
}

/// The Deep Searcher's family, the ladder's champion, which `beryl_cli ladder` plays at the
/// proxy budget unless told otherwise (docs/ai-strategy.md §4).
pub const DEEP_SEARCHER: Family = Family {
    method: Method::Pimc { worlds: 72 },
    sight: Sight::Hidden,
    leaf: Leaf::Rollout,
    knowledge: Knowledge::Tuned,
    options: ANCHORED,
};

/// The options of the deep searches offered: the anchor alone.
const ANCHORED: Options = Options {
    anchor: true,
    guard: false,
    mid: false,
    means: false,
};

/// `pimc18/omniscient/rollout/tuned@30M`, Deep Seer as docs/incidents/2026-11-06-deep-seer-misplays.md
/// found it.
fn deep_seer_unanchored(t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn GameAgent + Send> {
    deep(
        t,
        seed,
        Method::Pimc { worlds: 18 },
        Sight::Omniscient,
        Options::default(),
    )
}

/// A long-think search of full rollouts and tuned card values by `method` and `sight`, with
/// `options`, at 30M engine steps a decision, on every core.
fn deep(
    t: &GameTable,
    seed: u64,
    method: Method,
    sight: Sight,
    options: Options,
) -> Box<dyn GameAgent + Send> {
    let family = Family {
        method,
        sight,
        leaf: Leaf::Rollout,
        knowledge: Knowledge::Tuned,
        options,
    };
    deep_family(t, seed, family)
}

/// A long-think search of `family` at 30M engine steps a decision, on every core.
fn deep_family(t: &GameTable, seed: u64, family: Family) -> Box<dyn GameAgent + Send> {
    let settings = Settings {
        family,
        work: 30_000_000,
        threads: std::thread::available_parallelism()
            .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)),
    };
    Box::new(Widening::new(LongSearch::<Small>::new(t, settings, seed)))
}

/// The agent registered under `name`, offered or retired.
pub fn agent(name: &str) -> Option<&'static AgentInfo> {
    AGENTS.iter().chain(RETIRED).find(|a| a.name == name)
}
