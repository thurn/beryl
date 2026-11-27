//! Queries: what the rules ask about an object, answered once.
//!
//! Implements: CR 108.4a, CR 202.3, CR 712.6d
//!
//! A slot holds the game index of a definition and the face that is up. Neither is an answer:
//! a token's definition is no card, a face-down permanent's says nothing of what it is, a
//! copy of a back face has mana value 0, and a controller is a layer result on the
//! battlefield and the owner in a library. So no other crate reads the index: it asks one of
//! these, each named for its rule.
//!
//! [`crate::Observation::card`] is the other door, for a player's view: nothing when the
//! viewer does not know the card.

use beryl_card::definition::Shape;
use beryl_card::{AbilityIx, CardDef, Colors, FaceIx, Keywords, Subtype, Supertypes, Types, Zone};

use crate::bitboard::Bitboard;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::object::{AbilityMask, CombatRole, Flag};
use crate::reference::Permanent;
use crate::state::{StackEntry, State};
use crate::table::{Capability, GameTable};

pub(crate) mod defined_power;
pub mod zone_characteristics;

pub use crate::copiable::{Copiable, DefinitionKey};

impl<L: Layout> State<L> {
    /// The card that represents the object in `slot`: nothing for a token (see [CR 111.1]): "a marker
    /// used to represent any permanent that isn't represented by a card", which stays one wherever
    /// it goes (see [CR 111.7]) and is never a card (see [CR 111.6]), whatever it copies. Ask this for what
    /// only a card has: the other faces it could be cast or played with, what turning a face-down
    /// permanent face up shows, a place in a decklist.
    ///
    /// It is the rules' answer, with nothing masked: a face-down permanent's is its hidden
    /// card. A card under a copy effect answers what it copied, whose faces it has while it
    /// stays; [`State::card_really_is`] names the card itself. A player's view asks
    /// [`crate::Observation::card`].
    ///
    /// [CR 111.1]: https://yawgatog.com/resources/magic-rules/#R1111
    /// [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
    /// [CR 111.6]: https://yawgatog.com/resources/magic-rules/#R1116
    #[inline]
    pub fn card_behind(&self, slot: L::Slot) -> Option<L::Card> {
        let o = self.object(slot);
        (!o.is_token()).then_some(o.card)
    }

    /// The game index of the card or token in `slot` itself: its definition's, or for a card
    /// under a copy effect the card it really is ([`crate::Memo::Really`]), which it is again
    /// once it leaves the battlefield. For what names an object apart from its values: a
    /// deck's cards, and what a tool or a save calls it.
    #[inline]
    pub fn card_really_is(&self, slot: L::Slot) -> L::Card {
        let o = self.object(slot);
        // One test for an object that is no copy.
        if o.flags.has(Flag::Copy) && !o.is_token() {
            self.really_is(slot)
        } else {
            o.card
        }
    }

    /// The definition of the card that represents the object in `slot`, with every face it may be
    /// cast or played with (see [CR 712.6a]); nothing for a token ([`State::card_behind`]).
    ///
    /// [CR 712.6a]: https://yawgatog.com/resources/magic-rules/#R7126a
    #[inline]
    pub fn card_def(&self, t: &GameTable, slot: L::Slot) -> Option<&'static CardDef> {
        self.card_behind(slot).map(|card| t.def(card))
    }

    /// The name of the card or token definition behind `slot`, whichever face is up or down:
    /// what orders a batch of objects so that no seed decides it (the contract's
    /// `in_game_order`), and what a tool, a save or a test calls an object. It is not the
    /// object's name by the rules, which is [`Copiable::name`], and it is the rules' own
    /// view: a face-down permanent's is its hidden card's, and a card under a copy effect's
    /// the card's it really is ([`State::card_really_is`]).
    #[inline]
    pub fn definition_name(&self, t: &GameTable, slot: L::Slot) -> &'static str {
        t.def(self.card_really_is(slot)).name
    }

    /// The copiable values of the object in `slot` (see CR 707.2): what it is before any other
    /// effect, and what a copy of it would be. Its definition and face are fields of the
    /// object. A split card outside the stack has the values of both halves (see CR 709.4).
    /// Exceptions from copy effects are read behind [`Flag::Copy`].
    #[inline]
    pub fn copiable(&self, slot: L::Slot) -> Copiable<L> {
        let o = self.object(slot);
        let mut values = Copiable::of_object(o);
        if o.zone != Zone::Stack && !o.on_battlefield() {
            values = values.with_other_half();
        }
        if o.flags.has(Flag::Copy) {
            values.with_additions_of(self.copy_effect(slot))
        } else {
            values
        }
    }

    /// Whether the object in `slot` has the card name `name` (from its copiable values).
    #[inline]
    pub fn has_name(&self, t: &GameTable, slot: L::Slot, name: &str) -> bool {
        let o = self.object(slot);
        if o.on_battlefield() && !o.flags.has(Flag::Copy) {
            self.definition(slot).name(t) == name
        } else {
            self.copiable(slot).has_name(t, name)
        }
    }

    /// Whether a copy effect supplies the face in `slot` (see CR 712.6d). A merge uses
    /// the same store for the characteristics of its top component (see CR 731.2a).
    #[inline]
    pub(crate) fn has_copied_face(&self, slot: L::Slot) -> bool {
        self.object(slot).flags.has(Flag::Copy) && !self.copy_effect(slot).has_component_face()
    }

    /// The mana value of the object in `slot` ([CR 202.3]): "The mana value of an object is a
    /// number equal to the total amount of mana in its mana cost, regardless of color." A spell
    /// counts its X ([CR 202.3e]): "X is treated as the number chosen for it while the object is on
    /// the stack."
    ///
    /// Under [CR 712.6d], a nonmodal double-faced permanent with its back face up "has only the
    /// characteristics of its back face. However, its mana value is calculated using the mana cost
    /// of its front face. If a permanent is copying the back face of a nonmodal double-faced
    /// permanent (even if the object representing that copy is itself a double-faced permanent),
    /// the mana value of that permanent is 0." A token with a double-faced card's faces also
    /// copies the back face when it has it up (see [CR 707.8a]). A face-down permanent has no mana
    /// cost (see CR 708.2a), so 0.
    ///
    /// [CR 202.3]: https://yawgatog.com/resources/magic-rules/#R2023
    /// [CR 202.3e]: https://yawgatog.com/resources/magic-rules/#R2023e
    /// [CR 712.6d]: https://yawgatog.com/resources/magic-rules/#R7126d
    /// [CR 707.8a]: https://yawgatog.com/resources/magic-rules/#R7078a
    #[inline]
    pub fn mana_value(&self, t: &GameTable, slot: L::Slot) -> u8 {
        let o = self.object(slot);
        if o.zone == Zone::Stack
            && let Some(spell) = self.spell(self.object_ref(slot))
            && let Some(entry) = self.spell_entry(spell)
        {
            return spell_mana_value(t, entry);
        }
        let definition = t.def(o.card);
        // see CR 709.4b: "A split card’s colors and mana value are determined from its combined
        // mana cost." A spell on the stack used its chosen face above.
        if matches!(definition.shape, Shape::Split { .. })
            && o.zone != Zone::Stack
            && o.face != FaceIx::DOWN
        {
            // Printed split-card mana values are small vocabulary bounds and cannot overflow `u8`.
            return definition
                .front()
                .cost
                .mana_value()
                .saturating_add(definition.face(FaceIx::BACK).cost.mana_value());
        }
        let up = definition.face(o.face);
        if !up.uses_front_mana_value() {
            up.cost.mana_value()
        } else if o.is_token() || self.has_copied_face(slot) {
            0
        } else if up.is_melded() {
            self.melded_mana_value(t, slot)
        } else {
            t.def(o.card).front().cost.mana_value()
        }
    }

    /// The mana value of the melded permanent in `slot` (see [CR 713.5c]): "the object represented
    /// by those cards has only the characteristics of the combined back face, and its mana
    /// value is the sum of the mana values of its front faces." Its own card is the one it
    /// really is ([`crate::Memo::Really`]), and the other is under it ([`State::pile`]).
    ///
    /// [CR 713.5c]: https://yawgatog.com/resources/magic-rules/#R7135c
    #[cold]
    #[inline(never)]
    fn melded_mana_value(&self, t: &GameTable, slot: L::Slot) -> u8 {
        let front = |card: L::Card| t.def(card).front().cost.mana_value();
        // Mutated pile mana values are bounded by printed card costs and cannot overflow `u8`.
        self.pile(slot)
            .fold(front(self.card_really_is(slot)), |sum, card| {
                sum.saturating_add(front(self.object(card).card))
            })
    }

    /// Whether the object in `slot` has the ability at `ability` of its face up: the one
    /// question every reader of an ability asks, whatever kind it is (triggered, activated,
    /// static or mana). A permanent may have lost it to an ability-removing effect
    /// (see [CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing
    /// effects, and effects that say an object can't have an ability are applied." An object
    /// in any other zone has no layer results, and the cards' effects change only
    /// permanents, so it has every ability of its face.
    ///
    /// With no permanent missing an ability, as most games go, one test of a flag
    /// the layers store.
    ///
    /// # Panics
    ///
    /// In debug builds, for a permanent that lost an ability while the layer results are
    /// stale.
    ///
    /// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
    #[inline]
    pub fn has_ability(&self, slot: L::Slot, ability: AbilityIx) -> bool {
        !self.flags.abilities_lost || self.keeps(slot, ability)
    }

    /// Whether some continuous effect in force removes abilities, whether or not it removes
    /// any from a permanent now: the flag the layers store for [`State::has_ability`]. A
    /// reader that asks what an object would have on entering the battlefield
    /// (see [CR 614.12]) need look no further when it is false: "check the characteristics of
    /// the permanent as it would exist on the battlefield, taking into account ...
    /// continuous effects that already exist and would apply to the permanent."
    ///
    /// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
    #[inline]
    pub fn abilities_may_be_lost(&self) -> bool {
        self.flags.abilities_lost
    }

    /// Whether some permanent is face down (see [CR 708.2]): the flag the layers store as they
    /// look at each permanent's face. A reader that looks for face-down permanents, such as
    /// the special action that turns one face up, need not walk the battlefield when it is
    /// false, as in most games, including those whose cards could manifest one.
    ///
    /// # Panics
    ///
    /// In debug builds, while the layer results are stale.
    ///
    /// [CR 708.2]: https://yawgatog.com/resources/magic-rules/#R7082
    #[inline]
    pub fn any_face_down(&self) -> bool {
        debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
        self.flags.face_down
    }

    /// Whether the object in `slot` has every ability of its face up: [`State::has_ability`]
    /// asked of all of them at once, for a reader whose card table's facts stand for the
    /// whole face, as the mana solver's do.
    #[inline]
    pub fn has_every_ability(&self, slot: L::Slot) -> bool {
        !self.flags.abilities_lost || self.keeps_all(slot)
    }

    /// Whether the permanent in `slot` makes the mana its card table's facts say
    /// ([`crate::table::FaceFacts::mana`]): it has every ability of its face, no intrinsic
    /// mana ability of a basic land type but its face's (see [CR 305.7]), no granted mana
    /// ability (see [CR 613.1f]), and it is no merged permanent with the abilities of cards
    /// under its top card ([`crate::query::Copiable::merged_faces`]). With no effect on the
    /// battlefield removing abilities, changing land types, or granting a mana ability, and no
    /// merged permanent, as most games go, one test of a flag the layers store.
    ///
    /// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
    /// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
    #[inline]
    pub fn makes_card_mana(&self, t: &crate::table::GameTable, slot: L::Slot) -> bool {
        !self.flags.mana_changed || self.keeps_card_mana(t, slot)
    }

    /// [`State::makes_card_mana`] when some effect changed abilities, land types, or granted a
    /// mana ability, or a merged permanent is on the battlefield. Out of line: most games
    /// never come here.
    #[cold]
    #[inline(never)]
    fn keeps_card_mana(&self, t: &crate::table::GameTable, slot: L::Slot) -> bool {
        if self.has_mana_changes() {
            return false;
        }
        self.battlefield_position(slot).is_none_or(|i| {
            debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
            let d = self.derived[i];
            let merged = self.object(slot).flags.has(Flag::Copy)
                && self.copiable(slot).merged_face(0).is_some();
            let granted_mana = t.can(crate::table::Capability::GrantsMana)
                && self.has_granted_mana_row(t, d.granted);
            { d.lost }.is_empty()
                && { d.keywords }.intrinsic_mana() == 0
                && !merged
                && !granted_mana
        })
    }

    /// Whether any bit set in `bits` names a row of the table of grants whose ability is a
    /// mana ability.
    fn has_granted_mana_row(&self, t: &crate::table::GameTable, mut bits: L::AbilityMask) -> bool {
        let rows = self.grant_rows();
        while !bits.is_empty() {
            let row = bits.lowest();
            bits = bits.without_lowest();
            if rows[row].ability(t).is_some_and(crate::table::is_simple) {
                return true;
            }
        }
        false
    }

    /// The kinds of mana the intrinsic mana abilities of the basic land types of the object
    /// in `slot` make, a bit per [`beryl_card::ManaColor`] (see [CR 305.6]): "An object with the
    /// land card type and a basic land type has the intrinsic ability “{T}: Add [mana
    /// symbol],” even if the text box doesn't actually contain that text or the object has no
    /// text box." Those of its face it does not write out ([`crate::table::land_mana`]),
    /// unless it has lost all its abilities, with those its effects gave it in layer 4
    /// ([`beryl_card::Keywords::PLAINS_MANA`]). A land whose land types an effect set has
    /// lost all of its face's (see [CR 305.7]): "It loses all abilities generated from its rules
    /// text, its old land types".
    ///
    /// With no effect removing abilities or changing land types, as most games go, those of
    /// its face: the layers store that no row has lost an ability or gained a land type's
    /// mana, so the results are not read.
    ///
    /// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
    /// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
    #[inline]
    pub fn intrinsic_mana(&self, t: &crate::table::GameTable, slot: L::Slot) -> u8 {
        let printed = self.definition(slot).facts(t).land_mana;
        if !self.flags.mana_changed {
            return printed;
        }
        self.changed_intrinsic_mana(t, slot, printed)
    }

    /// [`State::intrinsic_mana`] when some effect removed abilities or changed land types,
    /// `printed` being those of the object's face. Out of line: most games never come here.
    #[cold]
    #[inline(never)]
    fn changed_intrinsic_mana(
        &self,
        t: &crate::table::GameTable,
        slot: L::Slot,
        printed: u8,
    ) -> u8 {
        // see CR 612.1: a text change may make its type line name other basic land types.
        let words = self.words_of(slot);
        let printed = if words.changes_subtypes() {
            crate::table::land_mana(self.definition(slot).face(t), &words)
        } else {
            printed
        };
        match self.battlefield_position(slot) {
            None => printed,
            Some(i) => {
                debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
                let d = self.derived[i];
                let kept = if { d.lost } == L::AbilityMask::ALL {
                    0
                } else {
                    printed
                };
                kept | { d.keywords }.intrinsic_mana()
            }
        }
    }

    /// [`State::has_every_ability`] when some permanent has lost an ability. Out of line:
    /// most games never come here.
    #[cold]
    #[inline(never)]
    fn keeps_all(&self, slot: L::Slot) -> bool {
        self.battlefield_position(slot).is_none_or(|i| {
            debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
            { self.derived[i].lost }.is_empty()
        })
    }

    /// Whether the merged permanent in `slot` has the abilities of the cards under its top
    /// card ([`crate::query::Copiable::merged_faces`]): it has, unless an effect removed all
    /// its abilities (see [CR 613.1f]). An effect that removes one ability names it on the top
    /// card's face, so it removes none of these.
    ///
    /// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
    #[inline]
    pub fn keeps_merged(&self, slot: L::Slot) -> bool {
        !self.flags.abilities_lost
            || self.battlefield_position(slot).is_none_or(|i| {
                debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
                let lost = self.derived[i].lost;
                lost != L::AbilityMask::ALL
            })
    }

    /// [`State::has_ability`] when some permanent has lost an ability. Out of line: most
    /// games never come here.
    #[cold]
    #[inline(never)]
    fn keeps(&self, slot: L::Slot, ability: AbilityIx) -> bool {
        self.battlefield_position(slot).is_none_or(|i| {
            debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
            !{ self.derived[i].lost }.contains(ability)
        })
    }

    /// Who controls the object in `slot`: a permanent's controller after continuous effects
    /// (see [CR 110.2]) or the player who put the spell on the stack (see [CR 112.2]):
    /// "A spell's controller is, by default, the player who put it on the stack."
    /// See [CR 601.2a] for casting: "that player becomes its controller". Anywhere else
    /// the object uses its owner ([CR 108.4a]): "If anything asks for the
    /// controller of a card that doesn't have one (because it's not a permanent or spell), use its
    /// owner instead."
    ///
    /// One read off the battlefield: the zone move gives an object back to its owner, and
    /// only becoming a spell or entering the battlefield names another controller
    /// ([`State::become_spell`], [`State::arrive`]).
    ///
    /// # Panics
    ///
    /// In debug builds, for a permanent while the layer results are stale.
    ///
    /// [CR 110.2]: https://yawgatog.com/resources/magic-rules/#R1102
    /// [CR 112.2]: https://yawgatog.com/resources/magic-rules/#R1122
    /// [CR 601.2a]: https://yawgatog.com/resources/magic-rules/#R6012a
    /// [CR 108.4a]: https://yawgatog.com/resources/magic-rules/#R1084a
    #[inline]
    pub fn controller(&self, slot: L::Slot) -> Seat {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).controller
        } else {
            o.base_controller
        }
    }

    /// The power of the object in `slot`: a permanent's after continuous effects (see [CR 613.4]), and
    /// elsewhere its copiable value ([`Copiable::power`]) or what a characteristic-defining
    /// ability defines, which works in every zone ([`crate::query::defined_power`]).
    ///
    /// [CR 613.4]: https://yawgatog.com/resources/magic-rules/#R6134
    #[inline]
    pub fn power(&self, t: &GameTable, slot: L::Slot) -> L::Pt {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).power
        } else {
            L::Pt::from(self.pt_elsewhere(t, slot).0)
        }
    }

    /// The colors of the object in `slot` (see [CR 105.2]): "An object can be one or more of the
    /// five colors, or it can be no color at all." A permanent's are its layer results.
    /// Elsewhere they are those of the face it has there, then the continuous effects that
    /// reach other zones ([`crate::query::zone_characteristics`]), read with one test while there are none.
    ///
    /// [CR 105.2]: https://yawgatog.com/resources/magic-rules/#R1052
    #[inline]
    pub fn colors(&self, t: &GameTable, slot: L::Slot) -> Colors {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).colors
        } else if !self.reaches_elsewhere() && t.can(Capability::Split) {
            self.copiable(slot).colors(t)
        } else if !self.reaches_elsewhere() && !o.flags.has(Flag::Copy) {
            self.definition(slot).face(t).colors
        } else if !self.reaches_elsewhere() {
            self.copiable_colors(t, slot)
        } else {
            self.elsewhere(t, slot).colors
        }
    }

    /// The card types of the object in `slot` (see [CR 205.2a]): "The card types are artifact,
    /// battle, conspiracy, creature, dungeon, enchantment, instant, kindred, land, phenomenon,
    /// plane, planeswalker, scheme, sorcery, and vanguard." Read as [`State::colors`] is.
    ///
    /// [CR 205.2a]: https://yawgatog.com/resources/magic-rules/#R2052a
    #[inline]
    pub fn types(&self, t: &GameTable, slot: L::Slot) -> Types {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).types
        } else if !self.reaches_elsewhere() {
            if t.can(Capability::Split) {
                self.copiable(slot).types(t)
            } else {
                self.definition(slot).face(t).types
            }
        } else {
            self.elsewhere(t, slot).types
        }
    }

    /// The supertypes of the object in `slot` (see [CR 205.4a]): "An object can have one or more
    /// supertypes." Read as [`State::colors`] is.
    ///
    /// [CR 205.4a]: https://yawgatog.com/resources/magic-rules/#R2054a
    #[inline]
    pub fn supertypes(&self, t: &GameTable, slot: L::Slot) -> Supertypes {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).supertypes
        } else if !self.reaches_elsewhere() {
            if t.can(Capability::Split) {
                self.copiable(slot).supertypes(t)
            } else {
                self.definition(slot).face(t).supertypes
            }
        } else {
            self.elsewhere(t, slot).supertypes
        }
    }

    /// Whether the object in `slot` has the subtype `subtype` (see [CR 205.3a]): "A card can have
    /// one or more subtypes printed on its type line." Read as [`State::colors`] is.
    ///
    /// [CR 205.3a]: https://yawgatog.com/resources/magic-rules/#R2053a
    #[inline]
    pub fn has_subtype(&self, t: &GameTable, slot: L::Slot, subtype: Subtype) -> bool {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).subtypes.contains(t.subtypes(), subtype)
        } else if !self.reaches_elsewhere() {
            if t.can(Capability::Split)
                && matches!(t.def(o.card).shape, Shape::Split { .. })
                && o.zone != Zone::Stack
            {
                return self
                    .copiable(slot)
                    .subtypes(t)
                    .contains(t.subtypes(), subtype);
            }
            let printed = self.definition(slot).face(t).subtypes;
            printed.read(&self.words_of(slot)).contains(subtype)
        } else {
            (self.elsewhere(t, slot).subtypes).contains(t.subtypes(), subtype)
        }
    }

    /// The keyword abilities of the object in `slot` that [`Keywords`] names: a permanent's
    /// after continuous effects (see [CR 613.1f]), and elsewhere those of the face it has there,
    /// which no effect of the cards changes off the battlefield. A split card off the stack has
    /// those of both halves (see CR 709.4c), read only in a game with a split card.
    ///
    /// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
    #[inline]
    pub fn keywords(&self, t: &GameTable, slot: L::Slot) -> Keywords {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).keywords
        } else if t.can(Capability::Split) {
            self.copiable(slot).keywords(t)
        } else {
            self.definition(slot).face(t).keywords
        }
    }

    /// Whether an effect that gives protection applied in the last run of the layers
    /// ([`crate::event::Replaces::PROTECTION`]), so that a permanent may have protection: the
    /// gate of every test of protection. One AND, with no test of whether the layer results
    /// are current, since protection is read from those results.
    #[inline]
    pub fn may_protect(&self) -> bool {
        self.replacing.protects()
    }

    /// The colors this permanent has protection from (see [CR 702.16a]). While no effect
    /// gives protection, this reads only [`crate::event::Replaces::PROTECTION`].
    ///
    /// [CR 702.16a]: https://yawgatog.com/resources/magic-rules/#R70216a
    #[inline]
    pub fn protection(&self, permanent: Permanent<L>) -> Colors {
        if !self.may_protect() {
            return Colors::NONE;
        }
        self.derived(permanent.slot()).protection
    }

    /// [`State::power`] for toughness.
    #[inline]
    pub fn toughness(&self, t: &GameTable, slot: L::Slot) -> L::Pt {
        let o = self.object(slot);
        if o.on_battlefield() {
            self.derived(slot).toughness
        } else {
            L::Pt::from(self.pt_elsewhere(t, slot).1)
        }
    }

    /// The objects another record of the state names, which are never interchangeable
    /// with a twin (architecture §7.2). [`State::signature`] compares what the state holds
    /// about an object itself; this is what it holds about something else that refers to
    /// the object, so that two objects alike in themselves lead to different games:
    ///
    /// - a stack entry names its spell's card or its ability's source, its targets
    ///   (see [CR 601.2c]: "The player
    ///   announces their choice of an appropriate object or player for each target the
    ///   spell requires."), and the object a triggered ability triggered on. An
    ///   ability's source is named though the ability exists apart from it: it can be its
    ///   own target, chosen before its costs are paid
    ///   (see [CR 602.2b]: "The remainder
    ///   of the process for activating an ability is identical to the process for casting
    ///   a spell listed in rules 601.2b-i"), so a land sacrificed to destroy target land
    ///   does nothing aimed at itself and destroys its twin aimed at that;
    /// - an ability that has triggered and waits to be put on the stack, or a delayed one,
    ///   names its source and that object
    ///   (see [CR 603.7c]: "A delayed
    ///   triggered ability that refers to a particular object still affects it even if the
    ///   object changes characteristics.");
    /// - a permanent names what it is attached to
    ///   (see [CR 701.3]), the planeswalker
    ///   it attacks and the attacker it blocks
    ///   (see [CR 509.1g]);
    /// - an entry of a store about one object names another, as a card in exile remembers
    ///   what exiled it (see [CR 610.3],
    ///   see [CR 607.2a]): the store says
    ///   which itself (`Store::names`).
    ///
    /// Every field and store that holds a slot is either about that slot's object, and
    /// then part of its signature, or one of these. A new one that refers to an object
    /// joins the walk here or its store's answer. The zones' orders, what each player
    /// knows and the open question of a machine step are no part of it: the first two are
    /// about the object, and the question names what it asks about, never a candidate
    /// still to choose.
    ///
    /// By slot, whichever object the record meant: one that outlived its object keeps
    /// apart two objects that might have been one candidate, and merges none that are not
    /// alike.
    ///
    /// [CR 601.2c]: https://yawgatog.com/resources/magic-rules/#R6012c
    /// [CR 602.2b]: https://yawgatog.com/resources/magic-rules/#R6022b
    /// [CR 603.7c]: https://yawgatog.com/resources/magic-rules/#R6037c
    /// [CR 701.3]: https://yawgatog.com/resources/magic-rules/#R7013
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    /// [CR 610.3]: https://yawgatog.com/resources/magic-rules/#R6103
    /// [CR 607.2a]: https://yawgatog.com/resources/magic-rules/#R6072a
    pub fn named(&self) -> Bitboard<L::Slot> {
        self.named_apart_from_waiting_triggers() | self.named_by_waiting_triggers()
    }

    /// [`State::named`] without what the abilities waiting to be put on the stack name:
    /// for the rule that compares those abilities with one another, to which each one's
    /// own source would otherwise never be like another's.
    pub fn named_apart_from_waiting_triggers(&self) -> Bitboard<L::Slot> {
        let mut named = self.store_names() | self.named_by_stack();
        // A set, built the same in any order. Only a permanent is attached or in combat
        // (the invariants hold an object off the battlefield to neither).
        for &slot in self.battlefield() {
            let o = self.object(slot);
            if let Some(host) = o.attached_to {
                named.insert(host);
            }
            if !matches!(o.combat, CombatRole::None) {
                for other in o.combat.names() {
                    named.insert(other);
                }
            }
        }
        named
    }
}

/// Mana value from a spell's chosen face and X, even when the entry is a snapshot.
/// See CR 202.3e: "X is treated as the number chosen for it while the object is on the stack."
#[must_use]
#[inline]
pub fn spell_mana_value<L: Layout>(table: &GameTable, entry: &StackEntry<L>) -> u8 {
    let cost = table.def(entry.card()).face(entry.face()).cost;
    // Mana value arithmetic is bounded by the vocabulary's `u8` cost fields.
    cost.mana_value()
        .saturating_add(cost.x.saturating_mul(entry.x))
}
