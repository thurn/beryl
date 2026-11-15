//! The abilities an object has now: the one query every rules decision that reads an
//! object's abilities asks ([`State::abilities`]).
//!
//! An object's abilities come from three places. Its face up has its own, which a permanent
//! may have lost to an ability-removing effect, a merged permanent has those of the cards
//! under its top card ([CR 702.140e]), and another object's effect may grant it more
//! ([CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing effects,
//! and effects that say an object can't have an ability are applied." A reader that walked the
//! face itself would miss the granted ones and must remember the lost ones, so no reader does:
//! each asks this query and gets them all, in one fixed order.
//!
//! [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
//! [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f

use beryl_card::{Ability, AbilityIx, FaceIx, ObjectAbility};

use crate::layout::Layout;
use crate::object::{Flag, GrantRef};
use crate::query::Copiable;
use crate::state::State;
use crate::table::{Capability, GameTable};

/// How many abilities other objects can grant one permanent: the length of
/// [`crate::object::Derived::granted`].
const GRANTS: usize = 4;

impl<L: Layout> State<L> {
    /// Every ability the object in `slot` has now, in a fixed order: the abilities of its
    /// face up that it has not lost, by their index on the face, then, of a merged permanent,
    /// those of the cards under its top card from the top of the pile down
    /// ([`Copiable::merged_faces`]), then those other objects granted it, in the order they
    /// were granted ([`crate::object::Derived::granted`]). Its
    /// face up is that of its copiable values ([`State::copiable`]), so a face-down
    /// permanent has none of its hidden card's. An object in a zone other than the
    /// battlefield has no layer results, so it has every ability of its face and none
    /// granted.
    ///
    /// It costs what walking the face costs: with no permanent missing an ability, as most
    /// games go, the face's abilities come with no test each, after one test of a flag the
    /// layers store, and in a game with no card that grants an ability
    /// ([`Capability::Grants`]) the granted part is skipped after one AND, as is the merged
    /// part in a game with no card that merges ([`Capability::Merges`]).
    ///
    /// # Panics
    ///
    /// In debug builds, for a permanent that lost or was granted an ability while the layer
    /// results are stale.
    #[inline]
    pub fn abilities<'s>(&'s self, t: &'s GameTable, slot: L::Slot) -> Abilities<'s, L> {
        let face = self.definition(slot).face(t).abilities;
        // In a game where nothing grants an ability, the granted part starts past its end.
        let (granted, next_grant) = if t.can(Capability::Grants) {
            (self.granted_to(slot), 0)
        } else {
            ([None; GRANTS], GRANTS as u8)
        };
        Abilities {
            state: self,
            slot,
            own: face.iter(),
            place: if t.can(Capability::Merges) {
                0
            } else {
                OWN_ONLY
            },
            index: 0,
            check_lost: self.flags.abilities_lost,
            granted,
            next_grant,
            table: t,
        }
    }

    /// Whether the object in `slot` has `ability` now ([`State::abilities`]): one of its own
    /// it has not lost ([`State::has_ability`]), or a granted one that is still granted.
    #[inline]
    pub fn has(&self, slot: L::Slot, ability: ObjectAbility) -> bool {
        match ability {
            ObjectAbility::Own(index) => self.has_ability(slot, index),
            ObjectAbility::Granted(k) => self
                .granted_to(slot)
                .get(usize::from(k))
                .is_some_and(Option::is_some),
            ObjectAbility::Merged(place, _) => self.has_merged(slot, place),
        }
    }

    /// Whether the object in `slot` is a merged permanent with a card at `place` under it whose
    /// abilities it has ([`State::has`]). Out of line: a game with no card that merges never
    /// asks.
    #[cold]
    #[inline(never)]
    fn has_merged(&self, slot: L::Slot, place: u8) -> bool {
        let o = self.object(slot);
        o.on_battlefield()
            && o.flags().has(Flag::Copy)
            && self.copiable(slot).merged_face(place).is_some()
            && self.keeps_merged(slot)
    }

    /// The values of the merged permanent in `slot` whose merged faces it has the abilities
    /// of ([`Copiable::merged_faces`]): none for an object that is not one, after a test of
    /// the game's capability in a game with no card that merges. Only a copy effect makes
    /// a merged permanent's values ([`State::merge`]), so an object with no copy effect is
    /// not one.
    #[inline]
    fn merged_values(&self, t: &GameTable, slot: L::Slot) -> Option<Copiable<L>> {
        if !t.can(Capability::Merges) {
            return None;
        }
        let o = self.object(slot);
        (o.on_battlefield() && o.flags().has(Flag::Copy) && self.keeps_merged(slot))
            .then(|| self.copiable(slot))
    }

    /// The card, face and ability that `ability`, an ability of a card under the merged
    /// permanent in `slot` ([`ObjectAbility::Merged`]), names, if the permanent has it now.
    pub fn merged_ability(
        &self,
        t: &GameTable,
        slot: L::Slot,
        ability: ObjectAbility,
    ) -> Option<(L::Card, FaceIx, AbilityIx, &'static Ability)> {
        let ObjectAbility::Merged(place, index) = ability else {
            return None;
        };
        let (card, face) = self.merged_values(t, slot)?.merged_face(place)?;
        let a = t.def(card).face(face).abilities.get(index.index())?;
        Some((card, face, index, a))
    }

    /// The cards and faces under the merged permanent in `slot` whose abilities it has now
    /// ([`Copiable::merged_faces`]), from the top of the pile down: none for an object that is
    /// not a merged permanent, or one that lost all its abilities. In a game with no card that
    /// merges, one test of the game's capability.
    #[inline]
    pub fn merged_faces_now<'s>(
        &'s self,
        t: &'s GameTable,
        slot: L::Slot,
    ) -> impl Iterator<Item = (L::Card, FaceIx)> + 's {
        self.merged_values(t, slot)
            .into_iter()
            .flat_map(move |values| values.merged_faces(t))
    }

    /// The abilities granted to the object in `slot`: none off the battlefield.
    fn granted_to(&self, slot: L::Slot) -> [Option<GrantRef<L>>; GRANTS] {
        if self.object(slot).on_battlefield() {
            self.derived(slot).granted
        } else {
            [None; GRANTS]
        }
    }
}

/// The abilities an object has now, from [`State::abilities`].
#[derive(Clone)]
pub struct Abilities<'s, L: Layout> {
    state: &'s State<L>,
    slot: L::Slot,
    /// The abilities yet to come of the face up, then of each merged face in turn.
    own: std::slice::Iter<'static, Ability>,
    /// The index on its face of the next of `own`.
    index: u8,
    /// Which face `own` walks: 0 or [`OWN_ONLY`] for the face up, then `k + 1` for the
    /// merged face at place `k` ([`Copiable::merged_faces`]), and [`DONE`] once none is
    /// left. [`OWN_ONLY`] is a game with no card that merges, where no merged face follows.
    place: u8,
    /// Some permanent lost an ability, so each of `own` is asked whether this one has it.
    check_lost: bool,
    granted: [Option<GrantRef<L>>; GRANTS],
    /// The place in `granted` of the next granted ability to look at.
    next_grant: u8,
    table: &'s GameTable,
}

/// [`Abilities::place`] in a game with no card that merges.
const OWN_ONLY: u8 = 0x80;

/// [`Abilities::place`] once no merged face is left.
const DONE: u8 = u8::MAX;

impl<L: Layout> Abilities<'_, L> {
    /// The next ability of a card under a merged permanent, in a game with a card that
    /// merges. Out of line: most objects are not merged permanents, and most games have no
    /// card that merges.
    #[cold]
    #[inline(never)]
    fn next_merged(&mut self) -> Option<(ObjectAbility, &'static Ability)> {
        if self.place == 0 && self.state.merged_values(self.table, self.slot).is_none() {
            self.place = DONE;
            return None;
        }
        loop {
            if self.place > 0
                && let Some(ability) = self.own.next()
            {
                let index = AbilityIx::new(self.index);
                self.index += 1;
                return Some((ObjectAbility::Merged(self.place - 1, index), ability));
            }
            let next = self.state.copiable(self.slot).merged_face(self.place);
            let Some((card, face)) = next.filter(|_| self.place + 1 < OWN_ONLY) else {
                self.place = DONE;
                return None;
            };
            self.place += 1;
            self.own = self.table.def(card).face(face).abilities.iter();
            self.index = 0;
        }
    }
}

impl<L: Layout> Iterator for Abilities<'_, L> {
    type Item = (ObjectAbility, &'static Ability);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.place & !OWN_ONLY == 0 {
            for ability in self.own.by_ref() {
                let index = AbilityIx::new(self.index);
                self.index += 1;
                if !self.check_lost || self.state.has_ability(self.slot, index) {
                    return Some((ObjectAbility::Own(index), ability));
                }
            }
        }
        if self.place < OWN_ONLY
            && let Some(merged) = self.next_merged()
        {
            return Some(merged);
        }
        while usize::from(self.next_grant) < GRANTS {
            let k = self.next_grant;
            self.next_grant += 1;
            if let Some(grant) = self.granted[usize::from(k)] {
                // A granted ability is printed on its granting card's front face (§9).
                let ability = &self.table.def(grant.card).front().grants[grant.grant.index()];
                return Some((ObjectAbility::Granted(k), ability));
            }
        }
        None
    }
}
