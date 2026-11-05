//! The agents (architecture §19.1): everything that plays the game, behind one trait.
//! Milestone 1 starts with the seeded [`RandomAgent`] that fuzzing plays.

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
