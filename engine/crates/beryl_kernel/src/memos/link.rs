//! The object a card in exile was exiled with ([`Memo::ExiledWith`]), or a permanent was put
//! onto the battlefield with ([`Memo::PutOntoBattlefieldWith`]): its slot and which of the
//! slot's objects it was.
//!
//! The interpreter reads it to find the objects an ability of that object refers to.
//!
//! Implements: none. Names the object for the rules' linked abilities to read.

use crate::ids::SlotId;
use crate::layout::Layout;
use crate::object::ObjectRef;
use crate::relayout::slot;

use super::Memo;

/// One object, by its slot and the low byte of its incarnation: a memo has room for a
/// byte beside the slot (the store's entry is at most 4 bytes), not for the whole
/// incarnation.
///
/// The slot alone would name whichever object is in it now. A card exiled with Knowledge
/// Pool stays exiled with that Pool after it leaves the battlefield and returns, and an
/// ability of the old Pool still on the stack refers to the cards it exiled and not to those
/// the new Pool exiles (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)):
/// "An object that moves from one zone to another becomes a new object with no memory of, or
/// relation to, its previous existence." The ability still exists without its source (see
/// [CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)): "Once activated or
/// triggered, an ability exists on the stack independently of its source."
///
/// A byte repeats every 256 incarnations of a slot. When a new object arrives in a slot, the
/// links whose byte is that object's end ([`crate::state::State::forget_links_to`]), so a
/// link never names an object it did not come from. An ability of an object 256 objects ago
/// in the same slot would lose its cards, which no game reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link<L: Layout = crate::layout::Small> {
    /// The slot of the object that exiled the card or put the permanent onto the battlefield.
    pub slot: L::Slot,
    generation: u8,
}

/// The incarnation's low byte. The truncation is the point: see [`Link`].
#[inline]
const fn generation(incarnation: u16) -> u8 {
    incarnation.to_le_bytes()[0]
}

impl<L: Layout> Link<L> {
    /// A link to `object`.
    #[inline]
    pub fn to(object: ObjectRef<L>) -> Self {
        Link {
            slot: object.slot,
            generation: generation(object.incarnation),
        }
    }

    /// Whether it is a link to `object`.
    #[inline]
    pub fn is(self, object: ObjectRef<L>) -> bool {
        self == Link::to(object)
    }

    /// Whether a link to an object with this incarnation, in the same slot, would read as
    /// this one.
    #[inline]
    pub(super) fn shares_generation(self, incarnation: u16) -> bool {
        self.generation == generation(incarnation)
    }

    /// The link in layout `M`: the same slot number and incarnation.
    pub(super) fn relayout<M: Layout>(self) -> Link<M> {
        Link {
            slot: slot(self.slot),
            generation: self.generation,
        }
    }
}

/// The slot and the generation, as a derived `Hash` would. A canonical hash reads the
/// generation as how long ago it was current, as it does an [`ObjectRef`]'s incarnation
/// ([`crate::repeats`]).
impl<L: Layout> std::hash::Hash for Link<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.slot.hash(h);
        let ago = crate::repeats::reference(self.slot.index(), u16::from(self.generation));
        generation(ago).hash(h);
    }
}

impl<L: Layout> Memo<L> {
    /// "Exiled with" `source` (see CR 607.2a): the memo of a card that an ability of
    /// `source` exiled.
    #[inline]
    pub fn exiled_with(source: ObjectRef<L>) -> Self {
        Memo::ExiledWith(Link::to(source))
    }
}
