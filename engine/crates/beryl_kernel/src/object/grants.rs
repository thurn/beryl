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

/// Bit in [`GrantRow::grant`] that marks a row as lending abilities from other cards
/// ([`beryl_card::Modification::Lend`]) rather than granting one from [`beryl_card::Face::grants`].
const LENT_BIT: u8 = 0x80;

impl<L: Layout> GrantRow<L> {
    /// A row for a static ability of `lender`, printed on `card`, that lends abilities from
    /// other cards: `lent_ix` is its zero-based place among `card`'s [`beryl_card::Modification::Lend`]
    /// parts across all faces.
    #[inline]
    pub fn lent(card: L::Card, lender: L::Slot, lent_ix: u8) -> Self {
        debug_assert!(lent_ix < LENT_BIT, "at most 128 lending parts per card");
        GrantRow {
            card,
            grant: GrantIx::new(LENT_BIT | lent_ix),
            source: GrantSource::Permanent(lender),
        }
    }

    /// Whether this row lends abilities from other cards ([`beryl_card::Modification::Lend`])
    /// rather than granting one from [`beryl_card::Face::grants`].
    #[inline]
    pub fn is_lent(self) -> bool {
        self.grant.get() & LENT_BIT != 0
    }

    /// For a lent row, the zero-based place of its [`beryl_card::Modification::Lend`] part
    /// among `self.card`'s faces.
    #[inline]
    pub fn lent_index(self) -> Option<usize> {
        self.is_lent()
            .then(|| usize::from(self.grant.get() & !LENT_BIT))
    }

    /// For a lent row, the permanent whose static ability lends the abilities.
    #[inline]
    pub fn lender(self) -> Option<L::Slot> {
        match self.source {
            GrantSource::Permanent(slot) if self.is_lent() => Some(slot),
            GrantSource::Permanent(_) | GrantSource::Effect(_) => None,
        }
    }

    /// The ability the row grants, if it is a normal grant and not a lent row.
    #[inline]
    pub fn as_grant_ref(self) -> Option<GrantRef<L>> {
        (!self.is_lent()).then_some(GrantRef {
            card: self.card,
            grant: self.grant,
        })
    }

    /// The ability the row grants.
    #[inline]
    pub fn grant_ref(self) -> GrantRef<L> {
        debug_assert!(!self.is_lent(), "lent row has no GrantRef");
        GrantRef {
            card: self.card,
            grant: self.grant,
        }
    }

    /// The granted ability printed on its granting card's front face, or `None` for a lent row.
    #[inline]
    pub fn ability(self, t: &crate::table::GameTable) -> Option<&'static beryl_card::Ability> {
        let grant = self.as_grant_ref()?;
        Some(&t.def(grant.card).front().grants[grant.grant.index()])
    }
}

const _: () = assert!(size_of::<GrantRow<Small>>() == 4);

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
