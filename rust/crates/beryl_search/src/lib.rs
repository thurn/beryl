//! The agents (architecture §19.1): everything that plays the game, behind one trait.
//! Milestone 1 starts with the seeded [`RandomAgent`] that fuzzing plays; [`AGENTS`] names
//! every agent a game can be played against.

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Decision, Observation};

/// A player: answers decisions from what its seat can see.
pub trait Agent {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice;
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

/// An agent a game can be played against, by name: the registry the app and the CLI choose
/// opponents from.
#[derive(Clone, Copy, Debug)]
pub struct AgentInfo {
    pub name: &'static str,
    /// One line for a human choosing an opponent.
    pub description: &'static str,
    /// Builds the agent from a seed, so a seed reproduces its games.
    pub new: fn(seed: u64) -> Box<dyn Agent + Send>,
}

/// Every agent, in the order a human is offered them.
pub static AGENTS: &[AgentInfo] = &[AgentInfo {
    name: "random",
    description: "Chooses uniformly at random among its legal options",
    new: random,
}];

fn random(seed: u64) -> Box<dyn Agent + Send> {
    Box::new(RandomAgent::new(seed))
}

/// The agent registered under `name`.
pub fn agent(name: &str) -> Option<&'static AgentInfo> {
    AGENTS.iter().find(|a| a.name == name)
}
