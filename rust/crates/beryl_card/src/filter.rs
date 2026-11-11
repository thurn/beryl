//! `Filter`: the predicates targets and affected sets are written in.

use crate::characteristics::{Subtype, Supertypes, Types};

/// Whose objects a filter accepts, relative to the player the filter is evaluated for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Whose {
    You,
    Opponent,
}

/// A predicate over objects. Evaluating one is a pass over a candidate set; the hot ones
/// become ANDs over per-slot attribute bitboards as the cards that need them arrive.
///
/// A permanent's characteristics and controller are its derived ones, after continuous
/// effects; elsewhere they are the face's.
#[derive(Clone, Copy, Debug)]
pub enum Filter {
    Any,
    /// Has at least one of these card types.
    Types(Types),
    Supertypes(Supertypes),
    Subtype(Subtype),
    /// Controlled by (on the battlefield or stack) or owned by (elsewhere) this player
    /// ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)): "Only objects on the
    /// stack or on the battlefield have a controller."
    Whose(Whose),
    Tapped,
    ManaValueAtMost(u8),
    /// Mana value X or less, X being the value of the spell or ability whose target this is
    /// (Thieving Skydiver's "target artifact with mana value X or less").
    ManaValueAtMostX,
    Not(&'static Filter),
    All(&'static [Filter]),
    AnyOf(&'static [Filter]),
}
