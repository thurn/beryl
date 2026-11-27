//! The abilities an object has now: the one query every rules decision that reads an
//! object's abilities asks ([`State::abilities`]).
//!
//! Implements: none. Reads the abilities supplied by copiable values and layer results.
//!
//! An object's abilities come from three places. Its face up has its own, which a permanent
//! may have lost to an ability-removing effect, a merged permanent has those of the cards
//! under its top card (see [CR 702.140e]), and another object's effect may grant it more
//! (see [CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing effects,
//! and effects that say an object can't have an ability are applied." A reader that walked the
//! face itself would miss the granted ones and must remember the lost ones, so no reader does:
//! each asks this query and gets them all, in one fixed order.
//!
//! [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
//! [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f

use beryl_card::{
    Ability, AbilityIx, AbilityKind, FaceIx, Lends, Lent, LentFrom, Modification, ObjectAbility,
    Zone,
};

use crate::copiable::defines_pt;
use crate::filter::Evaluate;
use crate::layout::{Layout, Queue};
use crate::memos::LinkKind;
use crate::object::{AbilityMask, Flag, GrantRef, GrantRow};
use crate::query::Copiable;
use crate::state::State;
use crate::table::{Capability, GameTable};

impl<L: Layout> State<L> {
    /// Every ability the object in `slot` has now, in a fixed order: the abilities of its
    /// face up that it has not lost, by their index on the face, then, of a split card off the
    /// stack, those of its other half numbered after them (see [CR 709.4c]: "A split card has
    /// each card type specified on either of its halves and each ability in the text box of
    /// each half"), then, of a merged permanent,
    /// those of the cards under its top card from the top of the pile down
    /// ([`Copiable::merged_faces`]), then those other objects granted it, in the order the
    /// layers granted them ([`crate::object::Derived::granted`]): the `k`th of them is
    /// [`ObjectAbility::Granted`]`(k)`. Its
    /// face up is that of its copiable values ([`State::copiable`]), so a face-down
    /// permanent has none of its hidden card's. An object in a zone other than the
    /// battlefield has no layer results, so it has every ability of its face and none
    /// granted.
    ///
    /// It costs what walking the face costs: with no permanent missing an ability, as most
    /// games go, the face's abilities come with no test each, after one test of a flag the
    /// layers store, and in a game with no card that grants an ability
    /// ([`Capability::Grants`]) the granted part is skipped after one AND, as are the other
    /// half and the merged part in a game with no split card and no card that merges
    /// ([`Capability::OtherFaceAbilities`]).
    ///
    /// # Panics
    ///
    /// In debug builds, for a permanent that lost or was granted an ability while the layer
    /// results are stale.
    ///
    /// [CR 709.4c]: https://yawgatog.com/resources/magic-rules/#R7094c
    #[inline]
    pub fn abilities<'s>(&'s self, t: &'s GameTable, slot: L::Slot) -> Abilities<'s, L> {
        let face = self.definition(slot).face(t).abilities;
        // In a game where nothing grants an ability, the granted part is empty, and the mask
        // is not read.
        let granted = if t.can(Capability::Grants) {
            self.granted_to(slot)
        } else {
            L::AbilityMask::default()
        };
        Abilities {
            state: self,
            slot,
            own: face.iter(),
            // A split card off the stack has its other half's abilities too (see CR 709.4c),
            // which the out-of-line part looks up in a game with a split card.
            place: if t.can(Capability::OtherFaceAbilities) {
                0
            } else {
                OWN_ONLY
            },
            index: 0,
            check_lost: self.flags.abilities_lost,
            granted,
            next_grant: 0,
            next_lent: 0,
            table: t,
        }
    }

    /// Whether any permanent in the game has lost an ability in layer 6.
    #[inline]
    pub fn any_abilities_lost(&self) -> bool {
        self.flags.abilities_lost
    }

    /// Whether the object in `slot` has `ability` now ([`State::abilities`]): one of its own
    /// it has not lost ([`State::has_ability`]), a `k`th granted one, one of a merged face, or
    /// one from a lent row of the table of grants.
    #[inline]
    pub fn has(&self, slot: L::Slot, ability: ObjectAbility) -> bool {
        match ability {
            ObjectAbility::Own(index) => self.has_ability(slot, index),
            ObjectAbility::Granted(k) => self.granted_row(slot, k).is_some(),
            ObjectAbility::Merged(place, _) => self.has_merged(slot, place),
            ObjectAbility::Lent(row, _) => self.has_lent_row(slot, row),
        }
    }

    /// Whether the object in `slot` has a lent row at `row` of the table of grants
    /// ([`State::has`]). Out of line: a game with no lending card never asks.
    #[cold]
    #[inline(never)]
    fn has_lent_row(&self, slot: L::Slot, row: u8) -> bool {
        let row_ix = usize::from(row);
        self.granted_to(slot).has_row(row_ix)
            && self
                .grants
                .as_slice()
                .get(row_ix)
                .is_some_and(|r| r.is_lent())
    }

    /// Whether the object in `slot` has any row in the table of grants (granted or lent).
    #[inline]
    pub fn has_any_grant(&self, slot: L::Slot) -> bool {
        !self.granted_to(slot).is_empty()
    }

    /// The card, face, ability index and ability that `ability`, an ability lent to the
    /// permanent in `slot` ([`ObjectAbility::Lent`]), names, if the permanent has it now.
    #[inline(never)]
    pub fn lent_ability(
        &self,
        t: &GameTable,
        slot: L::Slot,
        ability: ObjectAbility,
    ) -> Option<(L::Card, FaceIx, AbilityIx, &'static Ability)> {
        let ObjectAbility::Lent(row, k) = ability else {
            return None;
        };
        let row_ix = usize::from(row);
        if !self.granted_to(slot).has_row(row_ix) {
            return None;
        }
        let grant_row = *self.grants.as_slice().get(row_ix)?;
        lent_in_row(self, t, grant_row, k)
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
        let (card, face, omits_pt) = self
            .merged_values(t, slot)?
            .merged_faces_with_pt_omission(t)
            .nth(usize::from(place))?;
        let a = t.def(card).face(face).abilities.get(index.index())?;
        if omits_pt && defines_pt(a) {
            return None;
        }
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

    /// The rows of the table of grants that grant the object in `slot` an ability, row `k`
    /// in bit `k`: none off the battlefield.
    fn granted_to(&self, slot: L::Slot) -> L::AbilityMask {
        if self.object(slot).on_battlefield() {
            self.derived(slot).granted
        } else {
            L::AbilityMask::default()
        }
    }

    /// The effects in effect that grant an ability, in the order their layer 6 changes
    /// applied, as the layers last applied: row `k` grants its ability to each permanent
    /// whose [`crate::object::Derived::granted`] has bit `k`.
    #[inline]
    pub fn grant_rows(&self) -> &[GrantRow<L>] {
        self.grants.as_slice()
    }

    /// The row of the table of grants that the object in `slot`'s `k`th granted ability
    /// ([`ObjectAbility::Granted`]`(k)`) comes from, and its place in the table, if it has
    /// one. Lent rows are skipped.
    #[inline]
    pub fn granted_row(&self, slot: L::Slot, k: u8) -> Option<(usize, GrantRow<L>)> {
        let mut bits = self.granted_to(slot);
        let rows = self.grants.as_slice();
        let mut left = k;
        while !bits.is_empty() {
            let row = bits.lowest();
            bits = bits.without_lowest();
            if rows[row].is_lent() {
                continue;
            }
            if left == 0 {
                return Some((row, rows[row]));
            }
            left -= 1;
        }
        None
    }

    /// The ability the object in `slot`'s `k`th granted ability ([`ObjectAbility::Granted`])
    /// is, if it has one: the card it is printed on and its place among that card's grants.
    #[inline]
    pub fn granted_ability(&self, slot: L::Slot, k: u8) -> Option<GrantRef<L>> {
        self.granted_row(slot, k).map(|(_, row)| row.grant_ref())
    }

    /// The text-change words to read `ability` of `slot` through: for a granted ability from a
    /// permanent, that granting permanent's words; otherwise `slot`'s own words.
    #[inline]
    pub fn ability_words(&self, slot: L::Slot, ability: ObjectAbility) -> beryl_card::Words {
        if let ObjectAbility::Granted(k) = ability
            && let Some((_, row)) = self.granted_row(slot, k)
            && let crate::object::GrantSource::Permanent(grantor) = row.source
        {
            return self.words_of(grantor);
        }
        self.words_of(slot)
    }
}

/// The [`Lent`] that a lent row of the table of grants names, by its zero-based place among
/// the lending card's [`Modification::Lend`] parts across all faces.
pub(crate) fn lent_spec<L: Layout>(t: &GameTable, row: GrantRow<L>) -> Option<&'static Lent> {
    let target = row.lent_index()?;
    let mut seen = 0usize;
    for face in t.def(row.card).faces {
        for ability in face.abilities {
            if let Some(spec) = ability.layer() {
                for modification in spec.modifications {
                    if let Modification::Lend(lent) = *modification {
                        if seen == target {
                            return Some(lent);
                        }
                        seen += 1;
                    }
                }
            }
        }
    }
    None
}

/// The `k`th ability lent by `row`, counting the lending cards in [`State::in_game_order`]
/// and each card's [`AbilityKind::Activated`] and [`AbilityKind::Mana`] abilities in face order.
#[inline(never)]
fn lent_in_row<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    row: GrantRow<L>,
    k: u8,
) -> Option<(L::Card, FaceIx, AbilityIx, &'static Ability)> {
    let lender = row.lender()?;
    let lent = lent_spec(t, row)?;
    let you = s.controller(lender);
    let words = s.words_of(lender);
    let (pool, filter) = match lent.from {
        LentFrom::ExiledWithThis(filter) => {
            (s.linked(LinkKind::ExiledWith, s.object_ref(lender)), filter)
        }
        LentFrom::Graveyards(filter) => (s.in_zone(Zone::Graveyard), filter),
    };
    let matching = pool.filter(|card_slot| filter.matches(s, t, card_slot, you, &words));
    if matching.is_empty() {
        return None;
    }
    let ordered = s.in_game_order(t, matching).ok()?;
    let mut remaining = usize::from(k);
    for &card_slot in ordered.as_slice() {
        let (card, face) = s.definition(card_slot).record(t);
        for (i, ability) in t.def(card).face(face).abilities.iter().enumerate() {
            let matches_kind = match lent.which {
                Lends::Activated => {
                    matches!(ability.kind(), AbilityKind::Activated | AbilityKind::Mana)
                }
            };
            if !matches_kind {
                continue;
            }
            if remaining == 0 {
                let index = AbilityIx::new(u8::try_from(i).expect("abilities under 256"));
                return Some((card, face, index, ability));
            }
            remaining -= 1;
        }
    }
    None
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
    /// Which face `own` walks: 0 or [`OWN_ONLY`] for the face up, [`HALF`] for the other half
    /// of a split card off the stack ([`Copiable::other_half_abilities`]), then `k + 1` for
    /// the merged face at place `k` ([`Copiable::merged_faces`]), and [`DONE`] once none is
    /// left. [`OWN_ONLY`] is a game with no card that merges and no split card, where no
    /// other half or merged face follows. The other half is a value of this byte rather than a
    /// field of its own, so that making and walking the iterator, which every caller inlines,
    /// does no more work in a game with no split card (a separate flag measured +0.07% on the
    /// count gate's rollouts).
    place: u8,
    /// Some permanent lost an ability, so each of `own` is asked whether this one has it.
    check_lost: bool,
    /// The rows of the table of grants yet to come that grant the object an ability, row `k`
    /// in bit `k` ([`State::granted_to`]).
    granted: L::AbilityMask,
    /// The `k` of [`ObjectAbility::Granted`] of the next of `granted`.
    next_grant: u8,
    /// The `k` of [`ObjectAbility::Lent`] of the lowest remaining lent row in `granted`.
    next_lent: u8,
    table: &'s GameTable,
}

/// [`Abilities::place`] in a game with no card that merges and no split card.
const OWN_ONLY: u8 = 0x80;

/// [`Abilities::place`] while `own` walks the other half of a split card off the stack,
/// which the out-of-line part walks, so it is not 0 and below [`OWN_ONLY`]. No merged face
/// has this place.
const HALF: u8 = OWN_ONLY - 1;

/// [`Abilities::place`] once no merged face is left.
const DONE: u8 = u8::MAX;

impl<L: Layout> Abilities<'_, L> {
    /// The next ability of the other half of a split card off the stack, in a game with a
    /// split card, or of a card under a merged permanent, in a game with a card that merges.
    /// Out of line: most objects are neither, and most games have no such card.
    #[cold]
    #[inline(never)]
    fn next_merged(&mut self) -> Option<(ObjectAbility, &'static Ability)> {
        // The other half of a split card off the stack goes on from the face's own: the card
        // has one text box with each half's abilities in it (see CR 709.4c).
        if self.place == 0 && self.table.can(Capability::Split) {
            self.own = (self.state.copiable(self.slot))
                .other_half_abilities(self.table)
                .iter();
            self.place = HALF;
        }
        if self.place == HALF {
            for ability in self.own.by_ref() {
                let index = AbilityIx::new(self.index);
                self.index += 1;
                if !self.check_lost || self.state.has_ability(self.slot, index) {
                    return Some((ObjectAbility::Own(index), ability));
                }
            }
            // The merged faces come next, as they do after the face's own.
            self.place = 0;
        }
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
                if defines_pt(ability)
                    && self
                        .state
                        .copiable(self.slot)
                        .merged_faces_with_pt_omission(self.table)
                        .nth(usize::from(self.place - 1))
                        .is_some_and(|(_, _, omits_pt)| omits_pt)
                {
                    continue;
                }
                return Some((ObjectAbility::Merged(self.place - 1, index), ability));
            }
            let next = self.state.copiable(self.slot).merged_face(self.place);
            let Some((card, face)) = next.filter(|_| self.place + 1 < HALF) else {
                self.place = DONE;
                return None;
            };
            self.place += 1;
            self.own = self.table.def(card).face(face).abilities.iter();
            self.index = 0;
        }
    }
}

impl<L: Layout> Abilities<'_, L> {
    /// The next ability other objects granted the object, from the lowest non-lent row left in
    /// `granted`, or, once all normal granted rows have been yielded, from the lent rows left
    /// in `granted` ([`Self::next_lent`]).
    #[inline(never)]
    fn next_granted(&mut self) -> Option<(ObjectAbility, &'static Ability)> {
        let rows = self.state.grants.as_slice();
        let mut scan = self.granted;
        while !scan.is_empty() {
            let row = scan.lowest();
            scan = scan.without_lowest();
            let grant = rows[row];
            if grant.is_lent() {
                continue;
            }
            self.granted = self.granted.without_row(row);
            let k = self.next_grant;
            self.next_grant += 1;
            // A granted ability is printed on its granting card's front face (§9).
            let ability = &self.table.def(grant.card).front().grants[grant.grant.index()];
            return Some((ObjectAbility::Granted(k), ability));
        }
        self.next_lent()
    }

    /// The next ability lent to the object from the lent rows left in `granted`. Out of line:
    /// only a game with a lending card comes here.
    #[inline(never)]
    fn next_lent(&mut self) -> Option<(ObjectAbility, &'static Ability)> {
        let rows = self.state.grants.as_slice();
        while !self.granted.is_empty() {
            let row = self.granted.lowest();
            let k = self.next_lent;
            if let Some((_, _, _, ability)) = lent_in_row(self.state, self.table, rows[row], k) {
                self.next_lent += 1;
                let row = u8::try_from(row).expect("grant rows under 256");
                return Some((ObjectAbility::Lent(row, k), ability));
            }
            self.granted = self.granted.without_lowest();
            self.next_lent = 0;
        }
        None
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
        if self.granted.is_empty() {
            return None;
        }
        self.next_granted()
    }
}
