//! The abilities an object has now: the one query every rules decision that reads an
//! object's abilities asks ([`State::abilities`]).
//!
//! An object's abilities come from two places. Its face up has its own, which a permanent
//! may have lost to an ability-removing effect, and another object's effect may grant it more
//! ([CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing effects,
//! and effects that say an object can't have an ability are applied." A reader that walked the
//! face itself would miss the granted ones and must remember the lost ones, so no reader does:
//! each asks this query and gets both, in one fixed order.
//!
//! [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f

use beryl_card::{Ability, AbilityIx, ObjectAbility};

use crate::layout::Layout;
use crate::object::GrantRef;
use crate::state::State;
use crate::table::{Capability, GameTable};

/// How many abilities other objects can grant one permanent: the length of
/// [`crate::object::Derived::granted`].
const GRANTS: usize = 4;

impl<L: Layout> State<L> {
    /// Every ability the object in `slot` has now, in a fixed order: the abilities of its
    /// face up that it has not lost, by their index on the face, then those other objects
    /// granted it, in the order they were granted ([`crate::object::Derived::granted`]). Its
    /// face up is that of its copiable values ([`State::copiable`]), so a face-down
    /// permanent has none of its hidden card's. An object in a zone other than the
    /// battlefield has no layer results, so it has every ability of its face and none
    /// granted.
    ///
    /// It costs what walking the face costs: with no permanent missing an ability, as most
    /// games go, the face's abilities come with no test each, after one test of a flag the
    /// layers store, and in a game with no card that grants an ability
    /// ([`Capability::Grants`]) the granted part is skipped after one AND.
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
        }
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
    own: std::slice::Iter<'static, Ability>,
    /// The index on the face of the next of `own`.
    index: u8,
    /// Some permanent lost an ability, so each of `own` is asked whether this one has it.
    check_lost: bool,
    granted: [Option<GrantRef<L>>; GRANTS],
    /// The place in `granted` of the next granted ability to look at.
    next_grant: u8,
    table: &'s GameTable,
}

impl<L: Layout> Iterator for Abilities<'_, L> {
    type Item = (ObjectAbility, &'static Ability);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for ability in self.own.by_ref() {
            let index = AbilityIx::new(self.index);
            self.index += 1;
            if !self.check_lost || self.state.has_ability(self.slot, index) {
                return Some((ObjectAbility::Own(index), ability));
            }
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
