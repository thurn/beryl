//! The one tree: nodes, relations, the solve order, paint order, hit testing, input routing, the `Feature` trait.

mod feature;
pub mod hit;
mod tree;

pub use feature::{Context, Effect, Feature, FeatureSlot};
pub use tree::{
    Anchor, Band, KeepClear, Node, NodeQuery, NodeRef, Show, SlotRef, SolvePhase, SolveStep, Space,
    Tree,
};
