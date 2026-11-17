//! Checked object references and sets for rules functions. See the crate's contract.
//! Implements: none. Validates and names objects of a particular kind.
//!

use core::marker::PhantomData;

use crate::bitboard::Bitboard;
use crate::ids::Seat;
use crate::layout::{Layout, Small};
use crate::object::Flag;
use crate::relayout::{Relayout, bits};
use crate::state::{ObjectRef, State, Zone, ZoneKey};

/// A card or token on the battlefield (see CR 110.1).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Permanent<L: Layout = Small>(ObjectRef<L>);

impl<L: Layout> Permanent<L> {
    /// The slot of this incarnation.
    #[inline]
    pub fn slot(self) -> L::Slot {
        self.0.slot
    }
}

/// A spell's object (see CR 112.1). Abilities have no object slot and are named by stack place.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Spell<L: Layout = Small>(ObjectRef<L>);

impl<L: Layout> Spell<L> {
    /// The slot of this incarnation.
    #[inline]
    pub fn slot(self) -> L::Slot {
        self.0.slot
    }
}

/// A card in one zone, identified by its own incarnation.
///
/// ```compile_fail,E0308
/// use beryl_kernel::reference::{CardIn, Graveyard, Permanent};
///
/// fn exile_for_cost(_: CardIn<Graveyard>) {}
/// fn pay_with_treasure(treasure: Permanent) {
///     exile_for_cost(treasure);
/// }
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CardIn<Z: ZoneMark, L: Layout = Small>(ObjectRef<L>, PhantomData<Z>);

impl<Z: ZoneMark, L: Layout> CardIn<Z, L> {
    /// The slot of this incarnation.
    #[inline]
    pub fn slot(self) -> L::Slot {
        self.0.slot
    }
}

/// A reference checked against the state immediately before an action.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Current<R>(R);

impl<R: Copy> Current<R> {
    /// The reference whose incarnation was checked.
    #[inline]
    pub fn reference(self) -> R {
        self.0
    }
}

/// Unordered slots selected as permanents. Use the set within the current instruction.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permanents<L: Layout = Small>(Bitboard<L::Slot>);

impl<L: Layout> Permanents<L> {
    /// The selected slots, for an event batch or an unconverted caller.
    #[inline]
    pub fn slots(self) -> Bitboard<L::Slot> {
        self.0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }
}

/// Unordered slots selected as cards of one zone. Use the set within the current instruction.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cards<Z: ZoneMark, L: Layout = Small>(Bitboard<L::Slot>, PhantomData<Z>);

impl<Z: ZoneMark, L: Layout> Cards<Z, L> {
    /// The selected slots, for an event batch or an unconverted caller.
    #[inline]
    pub fn slots(self) -> Bitboard<L::Slot> {
        self.0
    }

    /// Selects only cards whose slots are also in `picked`.
    #[inline]
    #[must_use]
    pub fn intersect(self, picked: Bitboard<L::Slot>) -> Self {
        Self(self.0 & picked, PhantomData)
    }
}

mod private {
    pub trait Sealed {}
}

/// An owner-indexed zone that holds cards outside the battlefield and stack.
pub trait ZoneMark: private::Sealed + Copy {
    const ZONE: Zone;
}

macro_rules! card_zones {
    ($($zone:ident),+ $(,)?) => {
        $(
            #[doc = concat!("The ", stringify!($zone), " zone.")]
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub struct $zone;

            impl private::Sealed for $zone {}

            impl ZoneMark for $zone {
                const ZONE: Zone = Zone::$zone;
            }
        )+
    };
}

card_zones!(Library, Hand, Graveyard, Exile);

impl<L: Layout> From<Permanent<L>> for ObjectRef<L> {
    #[inline]
    fn from(object: Permanent<L>) -> Self {
        object.0
    }
}

impl<L: Layout> From<Spell<L>> for ObjectRef<L> {
    #[inline]
    fn from(object: Spell<L>) -> Self {
        object.0
    }
}

impl<Z: ZoneMark, L: Layout> From<CardIn<Z, L>> for ObjectRef<L> {
    #[inline]
    fn from(card: CardIn<Z, L>) -> Self {
        card.0
    }
}

impl<L: Layout> Relayout for Permanent<L> {
    type In<M: Layout> = Permanent<M>;

    fn relayout<M: Layout>(&self) -> Self::In<M> {
        Permanent(self.0.relayout())
    }
}

impl<L: Layout> Relayout for Spell<L> {
    type In<M: Layout> = Spell<M>;

    fn relayout<M: Layout>(&self) -> Self::In<M> {
        Spell(self.0.relayout())
    }
}

impl<Z: ZoneMark, L: Layout> Relayout for CardIn<Z, L> {
    type In<M: Layout> = CardIn<Z, M>;

    fn relayout<M: Layout>(&self) -> Self::In<M> {
        CardIn(self.0.relayout(), PhantomData)
    }
}

impl<L: Layout> Relayout for Permanents<L> {
    type In<M: Layout> = Permanents<M>;

    fn relayout<M: Layout>(&self) -> Self::In<M> {
        Permanents(bits(self.0))
    }
}

impl<Z: ZoneMark, L: Layout> Relayout for Cards<Z, L> {
    type In<M: Layout> = Cards<Z, M>;

    fn relayout<M: Layout>(&self) -> Self::In<M> {
        Cards(bits(self.0), PhantomData)
    }
}

impl<L: Layout> State<L> {
    /// Checks an incarnation immediately before using the reference. A zone move ends the
    /// previous object (see [CR 400.7]): "An object that moves from one zone to another becomes
    /// a new object with no memory of, or relation to, its previous existence."
    ///
    /// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
    #[inline]
    pub fn current<R: Copy + Into<ObjectRef<L>>>(&self, reference: R) -> Option<Current<R>> {
        self.is_current(reference.into())
            .then_some(Current(reference))
    }

    /// Checks that the reference still names a phased-in permanent (see [CR 110.1]): "A permanent
    /// is a card or token on the battlefield." See [CR 702.26b] for phasing.
    ///
    /// [CR 110.1]: https://yawgatog.com/resources/magic-rules/#R1101
    /// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    #[inline]
    pub fn permanent(&self, reference: ObjectRef<L>) -> Option<Permanent<L>> {
        let object = self.object(reference.slot);
        (object.on_battlefield() && object.incarnation() == reference.incarnation)
            .then_some(Permanent(reference))
    }

    /// The phased-in permanent currently in this slot, if any.
    #[inline]
    pub fn permanent_at(&self, slot: L::Slot) -> Option<Permanent<L>> {
        let object = self.object(slot);
        object.on_battlefield().then_some(Permanent(ObjectRef {
            slot,
            incarnation: object.incarnation(),
        }))
    }

    /// Checks that the reference still names an object in the stack zone (see [CR 112.1]).
    /// Use [`State::take_off_stack`] to check for an entry available to remove.
    ///
    /// [CR 112.1]: https://yawgatog.com/resources/magic-rules/#R1121
    #[inline]
    pub fn spell(&self, reference: ObjectRef<L>) -> Option<Spell<L>> {
        let object = self.object(reference.slot);
        (object.zone() == Zone::Stack && object.incarnation() == reference.incarnation)
            .then_some(Spell(reference))
    }

    /// Checks that the reference still names a card in the requested zone (see [CR 111.6]).
    ///
    /// [CR 111.6]: https://yawgatog.com/resources/magic-rules/#R1116
    #[inline]
    pub fn card_in<Z: ZoneMark>(&self, reference: ObjectRef<L>, _: Z) -> Option<CardIn<Z, L>> {
        (self.is_card_in::<Z>(reference.slot) && self.is_current(reference))
            .then_some(CardIn(reference, PhantomData))
    }

    /// All phased-in permanents, without computing their characteristics.
    #[inline]
    pub fn permanents(&self) -> Permanents<L> {
        Permanents(self.battlefield().iter().copied().collect())
    }

    /// Filters the vocabulary's selected objects to phased-in permanents.
    #[inline]
    pub fn permanents_in(&self, selected: Bitboard<L::Slot>) -> Permanents<L> {
        Permanents(selected.filter(|slot| self.object(slot).on_battlefield()))
    }

    /// Takes the first remaining permanent in battlefield order (see [CR 613.7d]).
    /// A slot that is no longer on the battlefield is skipped.
    ///
    /// [CR 613.7d]: https://yawgatog.com/resources/magic-rules/#R6137d
    #[inline]
    pub fn next_permanent(&self, selected: &mut Permanents<L>) -> Option<Permanent<L>> {
        let &slot = self
            .battlefield()
            .iter()
            .find(|&&slot| selected.0.contains(slot))?;
        selected.0.remove(slot);
        Some(Permanent(self.object_ref(slot)))
    }

    /// Cards in a player's part of a zone. Tokens that will cease to exist at the next
    /// state-based check are excluded (see [CR 111.6]): "A token isn't a card"
    ///
    /// [CR 111.6]: https://yawgatog.com/resources/magic-rules/#R1116
    #[inline]
    pub fn cards_in<Z: ZoneMark>(&self, player: Seat, _: Z) -> Cards<Z, L> {
        let slots = self.zone(ZoneKey::Of(player, Z::ZONE));
        let cards = slots.filter(|slot| {
            let object = self.object(slot);
            !object.is_token() && !object.flags().has(Flag::Copy)
        });
        Cards(cards, PhantomData)
    }

    #[inline]
    fn is_card_in<Z: ZoneMark>(&self, slot: L::Slot) -> bool {
        let object = self.object(slot);
        object.zone() == Z::ZONE && !object.is_token() && !object.flags().has(Flag::Copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Sink;
    use crate::fixed::Full;
    use crate::ids::{Slot, SlotId};
    use crate::layout::Large;
    use crate::profile::Profile;
    use crate::zone::Place;

    #[test]
    fn a_token_in_the_graveyard_cannot_pay_a_card_cost() {
        let mut state = State::<Small>::empty(Profile::EXACT);
        let card = Slot::try_from_index(0).unwrap();
        let token = Slot::pool().next().unwrap();
        state.object_mut(token).flags.set(Flag::Token, true);
        for slot in [card, token] {
            state
                .move_object(&mut Sink::off(), slot, Zone::Graveyard, Place::Top)
                .unwrap();
        }
        let owner = state.object(card).owner();
        assert_eq!(
            state.cards_in(owner, Graveyard).slots(),
            Bitboard::single(card)
        );
        assert!(state.card_in(state.object_ref(token), Graveyard).is_none());

        let reference = state.card_in(state.object_ref(card), Graveyard).unwrap();
        let mut wide = state.widen();
        let slot = ObjectRef::from(reference).slot.to().unwrap();
        let wide_reference = reference.relayout::<Large>();
        assert!(wide.current(wide_reference).is_some());
        wide.move_object(&mut Sink::off(), slot, Zone::Exile, Place::Top)
            .unwrap();
        assert!(wide.current(wide_reference).is_none());
    }

    #[test]
    fn an_exhausted_incarnation_never_names_the_next_object() {
        let mut state = State::<Small>::empty(Profile::EXACT);
        let slot = Slot::try_from_index(0).unwrap();
        let original = state.object_ref(slot);
        state.object_mut(slot).incarnation = u16::MAX - 1;
        state
            .move_object(&mut Sink::off(), slot, Zone::Library, Place::Top)
            .unwrap();
        let current = state.object_ref(slot);
        let owner = state.object(slot).owner();
        assert_eq!(
            state.move_object(&mut Sink::off(), slot, Zone::Graveyard, Place::Top),
            Err(Full)
        );
        assert_eq!(state.shuffle_library(owner), Err(Full));
        assert_eq!(state.cease_to_exist(slot), Err(Full));
        assert_eq!(state.library(owner), &[slot]);
        assert!(state.current(current).is_some());
        assert!(state.current(original).is_none());
    }
}
