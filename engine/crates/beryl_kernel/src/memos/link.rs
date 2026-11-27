//! The object a card in exile was exiled with ([`Memo::ExiledWith`]) or until
//! ([`Memo::ExiledUntil`]), a permanent was put onto the battlefield with
//! ([`Memo::PutOntoBattlefieldWith`]), or a card went to exile together with
//! ([`Memo::ExiledFromPileOf`]): its slot and which of the slot's objects it was.
//!
//! The interpreter reads it to find the objects an ability of that object refers to.
//!
//! Implements: none. Names the object for the rules' linked abilities to read.

use crate::bitboard::Bitboard;
use crate::ids::SlotId;
use crate::layout::{Layout, Queue};
use crate::object::ObjectRef;
use crate::relayout::slot;

use super::{Kinds, Memo};

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
    /// The slot of the object the memo names.
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

/// How an object an ability moved is linked to the ability's source, for the source's other
/// abilities to find it by (see CR 607.2a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// A card in exile is "exiled with" the source ([`Memo::ExiledWith`]).
    ExiledWith,
    /// A permanent was put onto the battlefield with the source
    /// ([`Memo::PutOntoBattlefieldWith`]).
    PutOntoBattlefieldWith,
    /// A creature tapped to convoke the spell, or the permanent it became ([`Memo::Convoked`]).
    Convoked,
}

impl LinkKind {
    /// The bit of the memo's kind.
    #[inline]
    fn kinds(self) -> u32 {
        match self {
            LinkKind::ExiledWith => Kinds::EXILED_WITH,
            LinkKind::PutOntoBattlefieldWith => Kinds::PUT_WITH,
            LinkKind::Convoked => Kinds::CONVOKED,
        }
    }
}

impl<L: Layout> Memo<L> {
    /// The memo of an object linked to `source` as `kind` says.
    #[inline]
    pub fn linked(kind: LinkKind, source: ObjectRef<L>) -> Self {
        let link = Link::to(source);
        match kind {
            LinkKind::ExiledWith => Memo::ExiledWith(link),
            LinkKind::PutOntoBattlefieldWith => Memo::PutOntoBattlefieldWith(link),
            LinkKind::Convoked => Memo::Convoked(link),
        }
    }
}

impl<L: Layout> crate::state::State<L> {
    /// The objects linked to `source` as `kind` says: the cards exiled with it, or the
    /// permanents put onto the battlefield with it (see
    /// [CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
    /// refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." Kept after that object has left its
    /// zone, and never those of a new object in its slot ([`Link`]). With no object in the
    /// game linked this way, one AND.
    #[inline]
    pub fn linked(&self, kind: LinkKind, source: ObjectRef<L>) -> Bitboard<L::Slot> {
        (self.memos).remembering(kind.kinds(), Memo::linked(kind, source))
    }

    /// Ends the links to an earlier object in `slot` that would read as links to the new
    /// object arriving there, or to a freed token slot: the zone move calls it. The new object
    /// is not that object (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
    ///
    /// The cards exiled with an earlier object stay "exiled with" it
    /// (see [CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second
    /// ability refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." The permanents an earlier Aura put
    /// onto the battlefield stay put there with it. An ability of that object still on the
    /// stack finds them, and the link names that object alone ([`Link`]). Only a link whose
    /// incarnation byte is the new object's ends, so that it cannot read as a link to the
    /// new object. With no card in the game linked either way, one AND.
    #[inline]
    pub(crate) fn forget_links_to(&mut self, slot: L::Slot) {
        if self.memos.kinds.0 & Kinds::LINKS != 0 {
            self.forget_links_to_slot(slot);
        }
    }

    /// The rare path of [`State::forget_links_to`], out of line.
    #[cold]
    #[inline(never)]
    fn forget_links_to_slot(&mut self, slot: L::Slot) {
        let incarnation = self.object(slot).incarnation;
        let done = self.memos.end(|e| match e.memo {
            Memo::ExiledWith(link)
            | Memo::PutOntoBattlefieldWith(link)
            | Memo::ExiledUntil(link)
            | Memo::ExiledFromPileOf(link)
            | Memo::Convoked(link) => !(link.slot == slot && link.shares_generation(incarnation)),
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::Chosen(_)
            | Memo::Really(_)
            | Memo::PhasedOutUnder(_)
            | Memo::ControlledBy(_)
            | Memo::PartOf(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed
            | Memo::TurnsUpWhenUsed
            | Memo::CastFromGraveyard
            | Memo::ClassLevel(_)
            | Memo::Solved
            | Memo::CombatBand(_)
            | Memo::Prepared(_)
            | Memo::Cast => true,
        });
        self.settle_ended(done);
    }

    /// Re-points every [`Memo::Convoked`] naming `spell` at the `permanent` it resolved into.
    /// With no convoke memo in the game, one AND.
    #[inline]
    pub fn repoint_convoked(&mut self, spell: ObjectRef<L>, permanent: ObjectRef<L>) {
        if self.memos.kinds.0 & Kinds::CONVOKED != 0 {
            self.repoint_convoked_of(spell, permanent);
        }
    }

    /// The rare path of [`State::repoint_convoked`], out of line.
    #[cold]
    #[inline(never)]
    fn repoint_convoked_of(&mut self, spell: ObjectRef<L>, permanent: ObjectRef<L>) {
        let from = Memo::Convoked(Link::to(spell));
        let to = Memo::Convoked(Link::to(permanent));
        for e in self.memos.entries.as_mut_slice() {
            if e.memo == from {
                e.memo = to;
            }
        }
    }

    /// Ends every [`Memo::Convoked`] naming `spell` when it leaves the stack without becoming
    /// a permanent. With no convoke memo in the game, one AND.
    #[inline]
    pub(crate) fn forget_convoked(&mut self, spell: ObjectRef<L>) {
        if self.memos.kinds.0 & Kinds::CONVOKED != 0 {
            self.forget_convoked_of(spell);
        }
    }

    /// The rare path of [`State::forget_convoked`], out of line.
    #[cold]
    #[inline(never)]
    fn forget_convoked_of(&mut self, spell: ObjectRef<L>) {
        let target = Memo::Convoked(Link::to(spell));
        let done = self.memos.end(|e| e.memo != target);
        self.settle_ended(done);
    }
}
