//! The agents (architecture §19.1): everything that plays the game, behind one trait. The
//! seeded [`RandomAgent`] fuzzes; the [`GreedyAgent`] is the [`RolloutPolicy`] search plays
//! rollouts with; [`Mcts`] searches with it; the [`ExpertAgent`] plays by hand-written rules,
//! and the [`FrozenExpertAgent`] is a copy of it that never changes, the bar general agents
//! are measured against. [`AGENTS`] names every agent a game can be played against.

mod expert;
mod frozen;
mod mcts;
mod rollout;
mod values;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Decision, GameTable, Observation};

pub use expert::ExpertAgent;
pub use frozen::FrozenExpertAgent;
pub use mcts::{Budget, Mcts, Search, Sight, rollouts_run};
pub use rollout::{COUNTER_MIN, Inline, REMOVAL_MIN, RolloutPolicy, rollout};
pub use values::{CardInfo, CardValues};

/// A player: answers decisions from what its seat can see.
pub trait Agent {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice;
}

/// A boxed agent, as the registry builds them, plays like the agent inside.
impl<T: Agent + ?Sized> Agent for Box<T> {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
        (**self).choose(obs, d)
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

impl Agent for RandomAgent {
    fn choose(&mut self, _obs: Observation<'_>, d: &Decision) -> Choice {
        Choice(self.rng.below(d.len() as u64) as u8)
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

impl Agent for GreedyAgent {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
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
    pub new: fn(t: &GameTable, seed: u64, budget: Budget) -> Box<dyn Agent + Send>,
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
];

fn random(_t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn Agent + Send> {
    Box::new(RandomAgent::new(seed))
}

fn greedy(t: &GameTable, _seed: u64, _budget: Budget) -> Box<dyn Agent + Send> {
    Box::new(GreedyAgent::new(t))
}

fn expert(t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn Agent + Send> {
    Box::new(ExpertAgent::new(t, seed))
}

fn expert_frozen(t: &GameTable, seed: u64, _budget: Budget) -> Box<dyn Agent + Send> {
    Box::new(FrozenExpertAgent::new(t, seed))
}

fn mcts_omniscient(t: &GameTable, seed: u64, budget: Budget) -> Box<dyn Agent + Send> {
    Box::new(Mcts::new(t, Sight::Omniscient, budget, seed))
}

fn mcts_hidden(t: &GameTable, seed: u64, budget: Budget) -> Box<dyn Agent + Send> {
    Box::new(Mcts::new(t, Sight::Hidden, budget, seed))
}

/// The agent registered under `name`.
pub fn agent(name: &str) -> Option<&'static AgentInfo> {
    AGENTS.iter().find(|a| a.name == name)
}
