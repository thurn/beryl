//! `expert-frozen`: the expert as it was retuned for the cb8d5d5f mirror (bead by-2l8.1), a
//! fixed reference opponent for every general agent (docs/ai-strategy.md §3). Improving the
//! live expert or the greedy policy must not move the bar, so this is a copy of all the
//! expert plays with: its rules ([`expert`]), its combat planner, the rollout policy it falls
//! back to ([`rollout`]) and that policy's card values ([`values`]).
//!
//! Never tune it. Edit it only to follow a change of the engine's API, and only so that it
//! still plays the same: `tests/frozen.rs` pins its moves on seeded games.

mod expert;
mod rollout;
mod values;

pub use expert::ExpertAgent as FrozenExpertAgent;
