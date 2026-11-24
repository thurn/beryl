//! The agents' tests, in one binary: they play real cards through the crate's public API.
//! Only `allocations` has a binary of its own, because it installs a global allocator.

mod common;

mod agents;
mod estimate;
mod expert;
mod families;
mod frozen;
mod games;
mod greedy;
mod hold;
mod long;
mod mcts;
mod overlay;
mod rollout;
mod shrink;
mod turn;
mod waste;
