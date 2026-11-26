//! The agents' tests, in one binary: they play real cards through the crate's public API.
//! Only `allocations` has a binary of its own, because it installs a global allocator.

mod common;

mod agents;
mod card_shapes;
mod casting_and_combat;
mod decking;
mod estimate;
mod expert;
mod families;
mod forgetful_fish;
mod frozen;
mod game_structure;
mod games;
mod greedy;
mod hold;
mod long;
mod mcts;
mod outgrow;
mod overlay;
mod rollout;
mod shared;
mod shrink;
mod turn;
mod waste;
