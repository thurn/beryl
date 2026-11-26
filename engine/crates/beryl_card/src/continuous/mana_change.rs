//! Changes to the mana a permanent makes when it is tapped for mana ([`ManaChange`]).
//!
//! To tap a permanent for mana is to activate a mana ability of it that has `{T}` in its cost
//! ([CR 106.12](https://yawgatog.com/resources/magic-rules/#R10612)). A triggered ability that
//! triggers on that and could add mana is a triggered mana ability, which resolves immediately
//! without using the stack ([CR 605.1b](https://yawgatog.com/resources/magic-rules/#R6051b),
//! [CR 605.4a](https://yawgatog.com/resources/magic-rules/#R6054a)).

use crate::filter::Filter;
use crate::mana::ManaSet;

/// A change to the mana produced when a permanent is tapped for mana
/// ([CR 106.12](https://yawgatog.com/resources/magic-rules/#R10612)): either a triggered mana
/// ability that adds additional mana ([`ManaChange::Adds`],
/// [CR 605.1b](https://yawgatog.com/resources/magic-rules/#R6051b)) or a replacement effect
/// that multiplies the mana produced ([`ManaChange::Multiplies`],
/// [CR 106.12b](https://yawgatog.com/resources/magic-rules/#R10612b)).
#[derive(Clone, Copy, Debug)]
pub enum ManaChange {
    /// "Whenever a player taps [tapped] for mana, that player adds an additional [added]."
    Adds { tapped: Tapped, added: Added },
    /// "If you tap [tapped] for mana, it produces [by] times as much of that mana instead."
    /// Replaces the tapped ability's own product; does not multiply mana an [`Adds`](Self::Adds)
    /// change contributes.
    Multiplies { tapped: Filter, by: u8 },
}

/// Which permanent being tapped for mana triggers a [`ManaChange::Adds`].
#[derive(Clone, Copy, Debug)]
pub enum Tapped {
    /// Any permanent the filter accepts, whoever controls it ("a player taps an Island").
    /// A basic land type in the filter is read through the text changes of the source or the
    /// captured words of the resolving spell.
    A(Filter),
    /// The permanent this Aura is attached to ("enchanted land").
    AttachedTo,
}

/// The additional mana a [`ManaChange::Adds`] produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Added {
    /// Fixed mana (High Tide's `{U}`, Wild Growth's `{G}`).
    Of(ManaSet),
    /// One mana of any type that tap produced, chosen by the player when it produced more than
    /// one type (Mana Flare).
    TypeProduced,
}
