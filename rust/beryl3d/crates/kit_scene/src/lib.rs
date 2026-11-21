//! The one tree: nodes, relations, the solve order, paint order, hit testing, input routing, the `Feature` trait.

mod feature;
pub mod hit;
mod slot;
mod tree;

pub use feature::{Context, Effect, Feature, FeatureSlot};
pub use slot::{SizeClass, Slot};
pub use tree::{
    Anchor, Band, Candidate, KeepClear, Node, NodeQuery, NodeRef, ShapeAnchor, Show, ShowPlacement,
    SlotRef, SolvePhase, SolveStep, Space, Tree,
};
