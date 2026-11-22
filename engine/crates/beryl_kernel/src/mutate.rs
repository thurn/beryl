//! The object mutators: one for each thing the rules do to an object where it stands (the
//! crate's contract, "Mutators"). Each makes the whole change. One whose change a player can
//! see returns the [`Event`] it caused, for the rules' pipeline to settle.
//!
//! A mutator trusts its caller on what the rules decide (that a reference is current, that
//! a permanent is untapped before it taps) and keeps what the state must agree on: the
//! combat flags with the combat reference, a face with its timestamp and who knows it,
//! `derived_dirty` where the layers read the field.
//!
//! Implements: none. The mutators update object records for actions selected by the rules.

mod merge;

use beryl_card::definition::Shape;
use beryl_card::{CounterKind, FaceIx, Types, Zone};

use crate::copiable::additions::Addition;
use crate::effects::Continuous;
use crate::event::{Event, Recipient};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Table};
use crate::object::{Attachment, AttackTarget, CombatRole, Flag, ObjectFlags, ObjectRef, SlotSet};
use crate::query::Copiable;
use crate::reference::{Permanent, Spell};
use crate::state::State;
use crate::static_ability::Inputs;
use crate::store::Duration;
use crate::table::GameTable;

/// How a permanent enters the battlefield, as the rules decided once its replacement effects
/// applied (see [CR 614.12]): "Some replacement effects modify how a permanent enters the battlefield."
/// A record: the rules build one and [`State::arrive`] applies it.
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrival<L: Layout> {
    pub tapped: bool,
    /// Put onto the battlefield attacking this player (see [CR 508.4]).
    ///
    /// [CR 508.4]: https://yawgatog.com/resources/magic-rules/#R5084
    pub attacking: Option<Seat>,
    /// Under this player's control rather than its owner's.
    pub controller: Option<Seat>,
    /// The face up.
    pub face: FaceIx,
    /// Counters it enters with (see [CR 122.6]).
    ///
    /// [CR 122.6]: https://yawgatog.com/resources/magic-rules/#R1226
    pub counters: Option<(CounterKind, u8)>,
    /// The object or player an Aura attaches to as it enters (see [CR 608.3c]).
    ///
    /// [CR 608.3c]: https://yawgatog.com/resources/magic-rules/#R6083c
    pub attach: Option<Attachment<L::Slot>>,
}

impl<L: Layout> State<L> {
    /// Taps or untaps a permanent (see [CR 701.26a]): "To tap a permanent, turn it sideways from an
    /// upright position."
    ///
    /// The layer results are stale from here while a static ability on the battlefield asks
    /// whether a permanent is tapped ([`Inputs::TAPPED`]).
    ///
    /// [CR 701.26a]: https://yawgatog.com/resources/magic-rules/#R70126a
    #[inline]
    #[must_use]
    pub fn set_tapped(&mut self, permanent: Permanent<L>, tapped: bool) -> Event<L> {
        let object = ObjectRef::from(permanent);
        self.object_mut(permanent.slot())
            .flags
            .set(Flag::Tapped, tapped);
        if self.layer_inputs.contains(Inputs::TAPPED) {
            self.flags.derived_dirty = true;
        }
        if tapped {
            Event::Tap { object }
        } else {
            Event::Untap { object }
        }
    }

    /// Whether a permanent has not been under its controller's control continuously since their
    /// most recent turn began, as one a player just gained control of has not (see [CR 302.6]).
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    #[inline]
    pub fn set_summoning_sick(&mut self, permanent: Permanent<L>, sick: bool) {
        let slot = permanent.slot();
        self.object_mut(slot).flags.set(Flag::SummoningSick, sick);
    }

    /// The per-turn reset of every permanent as `active`'s turn begins: theirs have now been under
    /// their control since their most recent turn began (see [CR 302.6]). Reads each permanent's
    /// controller, so the layer results must be current. What the Trackers store noted of the turn
    /// before went as the turn began ([`State::begin_turn`]).
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    #[inline]
    pub fn reset_permanents_for_turn(&mut self, active: Seat) {
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            if self.controller(slot) == active {
                self.object_mut(slot).flags.set(Flag::SummoningSick, false);
            }
        }
    }

    /// Deals damage to a permanent, by its types as the layers have them.
    ///
    /// See [CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c): "Damage dealt to a
    /// planeswalker causes that many loyalty counters to be removed from that planeswalker."
    ///
    /// See [CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e): "Damage dealt to a creature
    /// by a source with neither wither nor infect causes that much damage to be marked on that
    /// creature."
    #[inline]
    #[must_use]
    pub fn mark_damage(
        &mut self,
        source: ObjectRef<L>,
        to: Permanent<L>,
        amount: u8,
        combat: bool,
    ) -> Event<L> {
        let types = self.derived(to.slot()).types;
        let o = self.object_mut(to.slot());
        if types.contains(Types::PLANESWALKER) {
            o.counters.loyalty = o.counters.loyalty.saturating_sub(amount);
        }
        if types.contains(Types::CREATURE) {
            o.damage = o.damage.saturating_add(amount);
        }
        Event::Damage {
            source,
            to: Recipient::Permanent(to.into()),
            amount,
            combat,
        }
    }

    /// Removes the damage marked on every permanent, as the cleanup step does (see [CR 514.2]): "all
    /// damage marked on permanents (including phased-out permanents) is removed".
    ///
    /// [CR 514.2]: https://yawgatog.com/resources/magic-rules/#R5142
    #[inline]
    pub fn clear_damage(&mut self) {
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            self.object_mut(slot).damage = 0;
        }
        if !self.phased_out.is_empty() {
            self.clear_phased_out_damage();
        }
    }

    /// Clears damage from the permanents kept outside the battlefield list during phasing.
    #[cold]
    #[inline(never)]
    fn clear_phased_out_damage(&mut self) {
        for slot in self.phased_out.iter() {
            self.object_mut(slot).damage = 0;
        }
    }

    /// Removes the damage marked on a permanent, as it regenerates (see [CR 120.6]):
    /// "All damage marked on a permanent is removed when it regenerates".
    ///
    /// [CR 120.6]: https://yawgatog.com/resources/magic-rules/#R1206
    #[inline]
    pub fn remove_damage(&mut self, permanent: Permanent<L>) {
        self.object_mut(permanent.slot()).damage = 0;
    }

    /// Turns the face-down permanent in `slot` face up. It stays the same object, with its
    /// counters, damage and status, and does not enter again (see [CR 708.8]): "As a face-down
    /// permanent is turned face up, its copiable values revert to its normal copiable values. Any
    /// effects that have been applied to the face-down permanent still apply to the face-up
    /// permanent. Any abilities relating to the permanent entering the battlefield don't trigger
    /// and don't have any effect, because the permanent has already entered the battlefield."
    ///
    /// Every player has seen it (see CR 701.40b), and it gets a new timestamp (see [CR 613.7f]): "A
    /// permanent receives a new timestamp each time it turns face up or face down."
    ///
    /// A flipped card's alternative characteristics apply again (see [CR 710.2]): "Once a
    /// permanent is flipped, its normal name, text box, type line, power, and toughness don’t
    /// apply and the alternative versions of those characteristics apply instead."
    ///
    /// [CR 708.8]: https://yawgatog.com/resources/magic-rules/#R7088
    /// [CR 613.7f]: https://yawgatog.com/resources/magic-rules/#R6137f
    /// [CR 710.2]: https://yawgatog.com/resources/magic-rules/#R7102
    #[inline]
    pub fn turn_face_up(&mut self, t: &GameTable, permanent: Permanent<L>) {
        let slot = permanent.slot();
        let timestamp = self.take_timestamp();
        let o = self.object_mut(slot);
        o.face = if o.flags.has(Flag::Flipped) && t.def(o.card).shape == Shape::Flip {
            FaceIx::BACK
        } else {
            FaceIx::FRONT
        };
        o.timestamp = timestamp;
        for k in self.knowledge.as_mut_slice() {
            k.insert(slot);
        }
        self.flags.derived_dirty = true;
    }

    /// Changes the face of the same permanent (see [CR 701.27c]): "A permanent that transforms
    /// doesn’t become a new object. Any effects that applied to that permanent will continue to
    /// apply to it." It gets a new timestamp (see [CR 613.7g]): "A double-faced permanent receives
    /// a new timestamp each time it transforms or converts."
    ///
    /// [CR 701.27c]: https://yawgatog.com/resources/magic-rules/#R70127c
    /// [CR 613.7g]: https://yawgatog.com/resources/magic-rules/#R6137g
    pub fn transform_permanent(&mut self, t: &GameTable, permanent: Permanent<L>, face: FaceIx) {
        let timestamp = self.take_timestamp();
        let object = self.object_mut(permanent.slot());
        object.face = face;
        object.timestamp = timestamp;
        self.mark_source_transformed(t, permanent);
        self.flags.derived_dirty = true;
    }

    /// Sets flipped status and selects the alternative characteristics when the permanent has
    /// them (see [CR 710.2]): "Once a permanent is flipped, its normal name, text box, type line,
    /// power, and toughness don’t apply and the alternative versions of those characteristics
    /// apply instead."
    ///
    /// [CR 710.2]: https://yawgatog.com/resources/magic-rules/#R7102
    pub fn flip_permanent(&mut self, permanent: Permanent<L>, use_alternative: bool) {
        let object = self.object_mut(permanent.slot());
        object.flags.set(Flag::Flipped, true);
        if use_alternative {
            object.face = FaceIx::BACK;
            self.flags.derived_dirty = true;
        }
    }

    /// Attaches the Equipment in `slot` to the permanent in `host`, both on the battlefield
    /// (see [CR 701.3a]): "To attach an Aura, Equipment, or Fortification to an object or player means
    /// to take it from where it currently is and put it onto that object or player." The rules
    /// decide that it may.
    ///
    /// It gets a new timestamp (see [CR 701.3c]): "Attaching an Aura, Equipment, or Fortification on
    /// the battlefield to a different object or player causes the Aura, Equipment, or Fortification
    /// to receive a new timestamp."
    ///
    /// [CR 701.3a]: https://yawgatog.com/resources/magic-rules/#R7013a
    /// [CR 701.3c]: https://yawgatog.com/resources/magic-rules/#R7013c
    #[inline]
    pub fn attach(&mut self, attachment: Permanent<L>, host: Permanent<L>) {
        self.attach_to(attachment, Attachment::Object(host.slot()));
    }

    /// Attaches an Aura to a player. The rules decide that it may.
    pub fn attach_to_player(&mut self, attachment: Permanent<L>, player: Seat) {
        self.attach_to(attachment, Attachment::Player(player));
    }

    fn attach_to(&mut self, attachment: Permanent<L>, host: Attachment<L::Slot>) {
        let timestamp = self.take_timestamp();
        let o = self.object_mut(attachment.slot());
        o.set_attachment(Some(host));
        o.timestamp = timestamp;
        self.flags.derived_dirty = true;
    }

    /// Unattaches the permanent in `slot` from what it is attached to, leaving it on the
    /// battlefield (see [CR 704.5n]): "If an Equipment or Fortification is attached to an illegal
    /// permanent or to a player, it becomes unattached from that permanent or player. It remains on
    /// the battlefield."
    ///
    /// [CR 704.5n]: https://yawgatog.com/resources/magic-rules/#R7045n
    #[inline]
    pub fn unattach(&mut self, permanent: Permanent<L>) {
        self.object_mut(permanent.slot()).set_attachment(None);
        self.flags.derived_dirty = true;
    }

    /// Makes the card just put on the stack the spell its caster chose: `face` up, under their
    /// control (see [CR 601.2a]): "It has all the characteristics of the card (or the copy of a card)
    /// associated with it, and that player becomes its controller." Face-down spells use the
    /// characteristics of [`FaceIx::DOWN`] (see CR 708.4). A fused spell uses
    /// [`FaceIx::FUSED`] (see [CR 702.102b]): "A fused split spell has the combined
    /// characteristics of its two halves."
    ///
    /// [CR 601.2a]: https://yawgatog.com/resources/magic-rules/#R6012a
    /// [CR 702.102b]: https://yawgatog.com/resources/magic-rules/#R702102b
    #[inline]
    pub fn become_spell(&mut self, spell: Spell<L>, face: FaceIx, controller: Seat) {
        let o = self.object_mut(spell.slot());
        o.face = face;
        o.base_controller = controller;
    }

    /// Readies a free slot of the token pool to be a token with the copiable values `what`: a
    /// predefined token's ([`Copiable::of_card`]) or a copy's (see [CR 707.2]: "the copy acquires the
    /// copiable values of the original object's characteristics"), so never a hidden card's
    /// ([`Copiable::record`]). see [CR 111.2]: "The player who creates a token is its owner." The token
    /// exists once it is moved onto the battlefield, where it enters with the face returned up
    /// (see [CR 707.8a]): "If the token is a copy of a double-faced permanent with its back face up,
    /// the token enters the battlefield with its back face up."
    ///
    /// Values with exceptions are a copy effect on the token ([`Continuous::Copy`]), which
    /// lasts as long as it exists, so the exceptions are part of its copiable values
    /// (see [CR 707.9b]): "The final set of values for that characteristic becomes part of the
    /// copiable values of the copy." [`Full`] if the Effects store has no room for it.
    ///
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    /// [CR 111.2]: https://yawgatog.com/resources/magic-rules/#R1112
    /// [CR 707.8a]: https://yawgatog.com/resources/magic-rules/#R7078a
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    #[inline]
    pub fn make_token(
        &mut self,
        t: &GameTable,
        slot: L::Slot,
        owner: Seat,
        what: Copiable<L>,
    ) -> Result<FaceIx, Full> {
        let (_, face) = what.record(t);
        self.ready_token(slot, owner, what.recorded(t))?;
        Ok(face)
    }

    /// [`State::make_token`] of values already as a record keeps them
    /// ([`Copiable::recorded`]).
    #[inline]
    pub(crate) fn ready_token(
        &mut self,
        slot: L::Slot,
        owner: Seat,
        values: Copiable<L>,
    ) -> Result<(), Full> {
        let o = self.object_mut(slot);
        debug_assert_eq!(o.zone, Zone::Nowhere);
        if o.incarnation == u16::MAX {
            return Err(Full);
        }
        o.card = values.card();
        o.owner = owner;
        o.base_controller = owner;
        o.flags = ObjectFlags::NONE;
        o.flags.set(Flag::Token, true);
        if values.is_excepted() {
            self.start_token_copy(slot, values)?;
        }
        Ok(())
    }

    /// Starts the copy effect a token copy made with exceptions is created with. Its source
    /// is the card whose exception was added last: the card whose ability made the copy.
    #[cold]
    #[inline(never)]
    fn start_token_copy(&mut self, slot: L::Slot, values: Copiable<L>) -> Result<(), Full> {
        let source = values
            .additions()
            .last()
            .map_or(values.card(), Addition::card);
        let copy = Continuous::Copy(self.object_ref(slot), values);
        self.start_effect(source, copy, Duration::Indefinite)
    }

    /// Starts the copy effect of the permanent that just entered in `slot` as a copy, with the
    /// copiable values `values` (see [CR 707.5]): "An object that enters the battlefield 'as a copy'
    /// or 'that's a copy' of another object becomes a copy as it enters the battlefield." Part
    /// of its zone change, before anything reads it: no event of its own. The effect lasts
    /// while the permanent stays, and ends with what the stores hold about it as it leaves; a
    /// card is then the card it really is again ([`crate::Memo::Really`]). Its source is the
    /// card it really is, whose ability made it a copy. [`Full`] if the stores had no room.
    ///
    /// [CR 707.5]: https://yawgatog.com/resources/magic-rules/#R7075
    #[cold]
    #[inline(never)]
    pub fn enter_as_copy(
        &mut self,
        permanent: Permanent<L>,
        values: Copiable<L>,
    ) -> Result<(), Full> {
        let slot = permanent.slot();
        let source = self.card_behind(slot).unwrap_or(self.object(slot).card);
        let copy = Continuous::Copy(permanent.into(), values);
        self.start_effect(source, copy, Duration::Indefinite)
    }

    /// Gives the permanent that just entered in `slot` the status it enters with. Part of
    /// its zone change: no event of its own, and the move has already marked the layer
    /// results stale. [`Full`] if it enters with counters of a kind the table has no room
    /// for a row of ([`crate::counters`]).
    #[inline]
    pub fn arrive(&mut self, permanent: Permanent<L>, how: Arrival<L>) -> Result<(), Full> {
        let slot = permanent.slot();
        debug_assert!(self.flags.derived_dirty);
        let o = self.object_mut(slot);
        o.flags.set(Flag::Tapped, how.tapped);
        if let Some(player) = how.attacking {
            o.combat = CombatRole::attacking(AttackTarget::Player(player));
        }
        o.face = how.face;
        if let Some(controller) = how.controller {
            o.base_controller = controller;
        }
        if let Some(host) = how.attach {
            o.set_attachment(Some(host));
        }
        match how.counters {
            Some((kind, n)) => self.set_counters(slot, kind, n),
            None => Ok(()),
        }
    }

    /// Makes a creature an attacking creature that attacks `player` (see [CR 508.1k]): "Each chosen
    /// creature still controlled by the active player becomes an attacking creature." It attacks
    /// that player until [`State::choose_attacked`] says otherwise.
    ///
    /// [CR 508.1k]: https://yawgatog.com/resources/magic-rules/#R5081k
    #[inline]
    pub fn declare_attacker(&mut self, attacker: Permanent<L>, player: Seat) {
        self.object_mut(attacker.slot()).combat =
            CombatRole::attacking(AttackTarget::Player(player));
    }

    /// Sets the target that a creature attacks (see [CR 506.2]): "that player, planeswalkers
    /// they control, and battles they protect may be attacked." When a planeswalker stops
    /// being attacked, set the target to [`AttackTarget::Removed`] (see [CR 506.4]): "A
    /// planeswalker or battle that's removed from combat stops being attacked." The creature
    /// stays an attacking creature (see [CR 506.4c]).
    ///
    /// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
    /// [CR 506.4]: https://yawgatog.com/resources/magic-rules/#R5064
    /// [CR 506.4c]: https://yawgatog.com/resources/magic-rules/#R5064c
    #[inline]
    pub fn choose_attacked(&mut self, attacker: Permanent<L>, target: AttackTarget<L>) {
        let CombatRole::Attacking {
            target: current, ..
        } = &mut self.object_mut(attacker.slot()).combat
        else {
            unreachable!("an attack target for a creature that is not attacking");
        };
        *current = target;
    }

    /// Makes a creature a blocking creature, whose attacker [`State::block`] names next
    /// (see [CR 509.1g]): "Each chosen creature still controlled by the defending player becomes a
    /// blocking creature."
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn declare_blocker(&mut self, blocker: Permanent<L>) {
        self.object_mut(blocker.slot()).combat = CombatRole::Blocking {
            attackers: SlotSet::EMPTY,
            struck_first: false,
        };
    }

    /// Has a blocking creature block `attacker`, which becomes blocked (see [CR 509.1h]): "An attacking
    /// creature with one or more creatures declared as blockers for it becomes a blocked creature".
    /// [`Full`] if the blocker already blocks as many attackers as a [`SlotSet`] holds.
    ///
    /// [CR 509.1h]: https://yawgatog.com/resources/magic-rules/#R5091h
    #[inline]
    pub fn block(&mut self, blocker: Permanent<L>, attacker: Permanent<L>) -> Result<(), Full> {
        let CombatRole::Blocking { attackers, .. } = &mut self.object_mut(blocker.slot()).combat
        else {
            unreachable!("a creature blocking that was not declared as a blocker");
        };
        attackers.insert(attacker.slot())?;
        let CombatRole::Attacking { blocked, .. } = &mut self.object_mut(attacker.slot()).combat
        else {
            unreachable!("a creature blocked that is not attacking");
        };
        *blocked = true;
        Ok(())
    }

    /// Records that a creature had first strike or double strike as the first combat damage
    /// step began. It assigns damage in the second step only if it then has double strike
    /// (see [CR 510.4]).
    ///
    /// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
    #[inline]
    pub fn mark_struck_first(&mut self, permanent: Permanent<L>) {
        match &mut self.object_mut(permanent.slot()).combat {
            CombatRole::Attacking { struck_first, .. }
            | CombatRole::Blocking { struck_first, .. } => {
                *struck_first = true;
            }
            CombatRole::None => unreachable!("a creature striking first that is not in combat"),
        }
    }

    /// Removes a permanent from combat (see [CR 506.4]): "A creature that's removed from combat stops
    /// being an attacking, blocking, blocked, and/or unblocked creature. A planeswalker or battle
    /// that's removed from combat stops being attacked." What attacked it or blocked it stays in
    /// combat, related to nothing.
    ///
    /// [CR 506.4]: https://yawgatog.com/resources/magic-rules/#R5064
    #[inline]
    pub fn remove_from_combat(&mut self, permanent: Permanent<L>) {
        let slot = permanent.slot();
        self.forget_combat_bands(Some(slot));
        self.object_mut(slot).combat = CombatRole::None;
        for i in 0..self.battlefield.len() {
            let other = self.battlefield.as_slice()[i];
            self.object_mut(other).combat.forget(slot);
        }
    }

    /// Removes every permanent from combat as it ends (see [CR 511.3]): "As soon as the end of combat
    /// step ends, all creatures, battles, and planeswalkers are removed from combat." The turn
    /// forgets that attackers were declared ([`State::declare_attack`]).
    ///
    /// [CR 511.3]: https://yawgatog.com/resources/magic-rules/#R5113
    #[inline]
    pub fn end_combat(&mut self) {
        self.turn.attacked = false;
        self.forget_combat_bands(None);
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            self.object_mut(slot).combat = CombatRole::None;
        }
    }
}
