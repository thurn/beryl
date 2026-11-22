//! Stable identifiers shared by scene nodes and layout relations.

/// A key supplied by the owner of a scene node and kept across rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementId(pub u32);
