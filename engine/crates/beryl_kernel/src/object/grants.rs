//! The abilities other objects grant a permanent: the rows of the state's table of grants and
//! the hash of the layer results' mask of them (architecture §9).
//!
//! Implements: none. Holds what the layers found granted.

use std::hash::Hash;

use beryl_card::GrantIx;

use super::{AbilityMask, Derived};
use crate::ids::CardId;
use crate::layout::{Layout, Small};

/// An ability one card grants to other objects: the card's game index and the index into its
/// front face's [`beryl_card::Face::grants`] (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GrantRef<L: Layout = Small> {
    pub card: L::Card,
    pub grant: GrantIx,
}

/// One row of the state's table of grants ([`crate::state::State::grant_rows`]): one effect
/// in effect that grants an ability, which the layers wrote as its layer 6 change applied. The
/// permanents it grants the ability to have its bit in their [`Derived::granted`]. Two
/// permanents whose static abilities grant the same ability are two rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GrantRow<L: Layout = Small> {
    /// The game index of the card the ability is printed on.
    pub card: L::Card,
    /// The index of the ability among its card's front face's [`beryl_card::Face::grants`].
    pub grant: GrantIx,
    /// Where the effect that grants it comes from.
    pub source: GrantSource<L>,
}

impl<L: Layout> GrantRow<L> {
    /// The ability the row grants.
    #[inline]
    pub fn grant_ref(self) -> GrantRef<L> {
        GrantRef {
            card: self.card,
            grant: self.grant,
        }
    }
}

/// Filler for the table's unused places.
impl<L: Layout> Default for GrantRow<L> {
    fn default() -> Self {
        GrantRow {
            card: L::Card::FIRST,
            grant: GrantIx::new(0),
            source: GrantSource::Effect(L::Card::FIRST),
        }
    }
}

/// Where the effect of a [`GrantRow`] comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GrantSource<L: Layout = Small> {
    /// A static ability of the permanent in this slot, or an exception of the copy effect
    /// that made its copiable values.
    Permanent(L::Slot),
    /// The effect of a resolved spell or ability from this card ([`crate::state::ContinuousEffect::source`]).
    Effect(L::Card),
}

/// The fields in order, as a derived `Hash` would, but that a permanent granted nothing
/// hashes its grants as the four empty slots they replaced did, so that a game with no
/// grant keeps the hash the benchmark pins. A permanent granted something hashes its mask,
/// and the state hashes the rows it names ([`crate::state::State`]'s `Hash`).
impl<L: Layout> Hash for Derived<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Derived {
            power,
            toughness,
            types,
            supertypes,
            colors,
            controller,
            protection,
            keywords,
            subtypes,
            granted,
            lost,
        } = *self;
        power.hash(h);
        toughness.hash(h);
        types.hash(h);
        supertypes.hash(h);
        colors.hash(h);
        controller.hash(h);
        protection.hash(h);
        keywords.hash(h);
        subtypes.hash(h);
        if granted.is_empty() {
            [None::<GrantRef<L>>; 4].hash(h);
        } else {
            granted.bits().hash(h);
        }
        lost.hash(h);
    }
}
