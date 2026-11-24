//! Storage metadata and layout conversion for a memo.
//! Implements: none. Describes how the store retains each memo.

use super::{Kinds, Layout, Memo, Zone, slot};

impl<L: Layout> Memo<L> {
    /// The bits of its kinds in a store's [`Kinds`].
    #[inline]
    pub(super) fn kinds(self) -> u32 {
        match self {
            Memo::Paid(paid) => Kinds::paid(paid),
            Memo::X(_) => Kinds::X,
            Memo::ExiledWith(_) => Kinds::EXILED_WITH,
            Memo::ExiledUntil(_) => Kinds::EXILED_UNTIL,
            Memo::Chosen(_) => Kinds::CHOSEN,
            Memo::Really(_) => Kinds::REALLY,
            Memo::PutOntoBattlefieldWith(_) => Kinds::PUT_WITH,
            Memo::PhasedOutUnder(_) => Kinds::PHASED_OUT_UNDER,
            Memo::PartOf(_) => Kinds::PART_OF,
            Memo::FaceDownUnder(_) => Kinds::FACE_DOWN_UNDER,
            Memo::Morphed => Kinds::MORPHED,
            Memo::ClassLevel(_) => Kinds::CLASS_LEVEL,
            Memo::Solved => Kinds::SOLVED,
            Memo::CombatBand(_) => Kinds::COMBAT_BAND,
            Memo::Prepared(_) => Kinds::PREPARED,
        }
    }

    /// The zone of an object that remembers it: a permanent remembers its spell, a card in
    /// exile what exiled it.
    #[inline]
    pub(super) fn zone(self) -> Zone {
        match self {
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::Chosen(_)
            | Memo::Really(_)
            | Memo::PutOntoBattlefieldWith(_)
            | Memo::PhasedOutUnder(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed
            | Memo::ClassLevel(_)
            | Memo::CombatBand(_)
            | Memo::Solved
            | Memo::Prepared(_) => Zone::Battlefield,
            Memo::ExiledWith(_) | Memo::ExiledUntil(_) => Zone::Exile,
            Memo::PartOf(_) => Zone::Merged,
        }
    }

    /// Whether the layers read it: a static ability may apply only while a cost was paid,
    /// as one with impending does
    /// (see [CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)).
    #[inline]
    pub(super) fn layer_input(self) -> bool {
        matches!(self, Memo::Paid(_) | Memo::ClassLevel(_) | Memo::Solved)
    }

    /// The slot it names beside the object that remembers it: what exiled the card. A
    /// kind that names one has its bit in [`Kinds::NAMING`], which the store's invariant
    /// holds it to.
    #[inline]
    pub(super) fn names(self) -> Option<L::Slot> {
        match self {
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::Chosen(_)
            | Memo::Really(_)
            | Memo::PhasedOutUnder(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed
            | Memo::ClassLevel(_)
            | Memo::CombatBand(_)
            | Memo::Solved => None,
            Memo::Prepared(copy) => copy,
            Memo::ExiledWith(link) => Some(link.slot),
            Memo::ExiledUntil(by) | Memo::PutOntoBattlefieldWith(by) | Memo::PartOf(by) => Some(by),
        }
    }

    /// Whether it says something, and of an exiler that is where such a memo needs it:
    /// what the store's invariant holds each memo to, given the zone of the object in a
    /// slot.
    pub(super) fn holds(self, zone: impl Fn(L::Slot) -> Zone) -> bool {
        match self {
            Memo::X(x) => x > 0,
            Memo::ClassLevel(level) => level > 1,
            Memo::Paid(paid) => !paid.is_empty(),
            Memo::Chosen(value) => value.holds(),
            // A card exiled with an object stays exiled with it after the object has left its
            // zone or, as a token, ceased to exist: an ability of it may still be on the stack
            // (see CR 113.7a).
            Memo::ExiledWith(_)
            | Memo::Really(_)
            | Memo::PhasedOutUnder(_)
            | Memo::Morphed
            | Memo::Solved
            | Memo::CombatBand(_) => true,
            // Somebody knew the face-down card: at least its controller (see CR 708.5).
            Memo::FaceDownUnder(knew) => !knew.is_empty(),
            // What put it onto the battlefield left a card or a token in its slot.
            Memo::PutOntoBattlefieldWith(by) => zone(by) != Zone::Nowhere,
            // An object exiled "until" its exiler leaves returns as it leaves (see CR 610.3).
            Memo::ExiledUntil(by) => zone(by) == Zone::Battlefield,
            // A card is part of a permanent only while it is on the battlefield.
            Memo::PartOf(of) => zone(of) == Zone::Battlefield,
            Memo::Prepared(copy) => {
                copy.is_none_or(|copy| matches!(zone(copy), Zone::Exile | Zone::Stack))
            }
        }
    }

    /// The memo in layout `M`: a slot it names at the same number.
    pub(super) fn relayout<M: Layout>(self) -> Memo<M> {
        match self {
            Memo::Paid(paid) => Memo::Paid(paid),
            Memo::X(x) => Memo::X(x),
            Memo::ExiledWith(link) => Memo::ExiledWith(link.relayout()),
            Memo::ExiledUntil(by) => Memo::ExiledUntil(slot(by)),
            Memo::Chosen(value) => Memo::Chosen(value.relayout()),
            Memo::Really(card) => Memo::Really(crate::relayout::card(card)),
            Memo::PutOntoBattlefieldWith(by) => Memo::PutOntoBattlefieldWith(slot(by)),
            Memo::PhasedOutUnder(player) => Memo::PhasedOutUnder(player),
            Memo::PartOf(of) => Memo::PartOf(slot(of)),
            Memo::FaceDownUnder(knew) => Memo::FaceDownUnder(knew),
            Memo::Morphed => Memo::Morphed,
            Memo::ClassLevel(level) => Memo::ClassLevel(level),
            Memo::Solved => Memo::Solved,
            Memo::CombatBand(band) => Memo::CombatBand(slot(band)),
            Memo::Prepared(copy) => Memo::Prepared(copy.map(slot)),
        }
    }

    /// Whether `other` is of the same variant: an object has one memo of each.
    #[inline]
    pub(super) fn same_variant(self, other: Memo<L>) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}
