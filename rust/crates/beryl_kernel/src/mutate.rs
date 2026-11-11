//! The object mutators: one for each thing the rules do to an object where it stands (the
//! crate's contract, "Mutators"). Each makes the whole change. One whose change a player can
//! see returns the [`Event`] it caused, for the rules' pipeline to settle.
//!
//! A mutator trusts its caller on what the rules decide (that a reference is current, that
//! a permanent is untapped before it taps) and keeps what the state must agree on: the
//! combat flags with the combat reference, a face with its timestamp and who knows it,
//! `derived_dirty` where the layers read the field.

use beryl_card::{CounterKind, FaceIx, Types, Zone};

use crate::event::{Event, Recipient};
use crate::ids::{CardIx, Seat, SlotId};
use crate::layout::{Layout, Queue};
use crate::object::{Counters, ObjectFlags, ObjectRef};
use crate::query::Copiable;
use crate::state::State;
use crate::table::GameTable;

/// How a permanent enters the battlefield, as the rules decided once its replacement effects
/// applied ([CR 614.12](https://yawgatog.com/resources/magic-rules/#R61412)): "Some
/// replacement effects modify how a permanent enters the battlefield." A record: the rules
/// build one and [`State::arrive`] applies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrival<L: Layout> {
    pub tapped: bool,
    /// Put onto the battlefield attacking
    /// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)).
    pub attacking: bool,
    /// Under this player's control rather than its owner's.
    pub controller: Option<Seat>,
    /// The face up.
    pub face: FaceIx,
    /// Counters of a common kind it enters with
    /// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)).
    pub counters: Option<(CounterKind, u8)>,
    /// The object an Aura attaches to as it enters
    /// ([CR 608.3c](https://yawgatog.com/resources/magic-rules/#R6083c)).
    pub attach: Option<L::Slot>,
}

impl<L: Layout> State<L> {
    /// Taps or untaps a permanent
    /// ([CR 701.26a](https://yawgatog.com/resources/magic-rules/#R70126a)): "To tap a
    /// permanent, turn it sideways from an upright position."
    #[inline]
    #[must_use]
    pub fn set_tapped(&mut self, object: ObjectRef<L>, tapped: bool) -> Event<L> {
        debug_assert!(self.is_current(object));
        self.object_mut(object.slot)
            .flags
            .set(ObjectFlags::TAPPED, tapped);
        if tapped {
            Event::Tap { object }
        } else {
            Event::Untap { object }
        }
    }

    /// Whether a permanent has not been under its controller's control continuously since
    /// their most recent turn began, as one a player just gained control of has not
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    #[inline]
    pub fn set_summoning_sick(&mut self, slot: L::Slot, sick: bool) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot)
            .flags
            .set(ObjectFlags::SUMMONING_SICK, sick);
    }

    /// The per-turn resets of every permanent as `active`'s turn begins: theirs have now been
    /// under their control since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)), and nothing has
    /// entered, nor has a loyalty ability been activated
    /// ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)), this turn. Reads each
    /// permanent's controller, so the layer results must be current.
    #[inline]
    pub fn reset_permanents_for_turn(&mut self, active: Seat) {
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            let controlled = self.controller(slot) == active;
            let o = self.object_mut(slot);
            if controlled {
                o.flags.set(ObjectFlags::SUMMONING_SICK, false);
            }
            o.flags.set(ObjectFlags::ENTERED_THIS_TURN, false);
            // One of the flags milestone 4 (by-w8gv.7.1) moves out, reset here so the turn's
            // beginning walks the battlefield once.
            o.flags.set(ObjectFlags::LOYALTY_ACTIVATED, false);
        }
    }

    /// Deals damage to a permanent, by its types as the layers have them.
    ///
    /// [CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c): "Damage dealt to a
    /// planeswalker causes that many loyalty counters to be removed from that planeswalker."
    ///
    /// [CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e): "Damage dealt to a
    /// creature by a source with neither wither nor infect causes that much damage to be
    /// marked on that creature."
    #[inline]
    #[must_use]
    pub fn mark_damage(
        &mut self,
        source: ObjectRef<L>,
        to: ObjectRef<L>,
        amount: u8,
        combat: bool,
    ) -> Event<L> {
        debug_assert!(self.is_current(to));
        let types = self.derived(to.slot).types;
        let o = self.object_mut(to.slot);
        if types.contains(Types::PLANESWALKER) {
            o.counters.loyalty = o.counters.loyalty.saturating_sub(amount);
        }
        if types.contains(Types::CREATURE) {
            o.damage = o.damage.saturating_add(amount);
        }
        Event::Damage {
            source,
            to: Recipient::Permanent(to),
            amount,
            combat,
        }
    }

    /// Removes the damage marked on every permanent, as the cleanup step does
    /// ([CR 514.2](https://yawgatog.com/resources/magic-rules/#R5142)): "all damage marked on
    /// permanents (including phased-out permanents) is removed".
    #[inline]
    pub fn clear_damage(&mut self) {
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            self.object_mut(slot).damage = 0;
        }
    }

    /// Puts counters of a common kind on an object
    /// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)). `layered` says
    /// whether the layers read them, which is the rules' to say, with the card in hand: the
    /// layer results are stale if so.
    #[inline]
    #[must_use]
    pub fn add_counters(
        &mut self,
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        debug_assert!(!Counters::is_long_tail(kind));
        self.put_counters(object, kind, amount, layered)
    }

    /// Removes `amount` counters of a common kind from an object that has that many.
    /// `layered` as for [`State::add_counters`].
    #[inline]
    #[must_use]
    pub fn remove_counters(
        &mut self,
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        debug_assert!(!Counters::is_long_tail(kind));
        self.take_counters(object, kind, amount, layered)
    }

    #[inline]
    pub(crate) fn put_counters(
        &mut self,
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        debug_assert!(self.is_current(object));
        let n = self.object_mut(object.slot).counters.get_mut(kind);
        *n = n.saturating_add(amount);
        if layered {
            self.flags.derived_dirty = true;
        }
        Event::CountersAdded {
            object,
            kind,
            amount,
        }
    }

    #[inline]
    pub(crate) fn take_counters(
        &mut self,
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
        layered: bool,
    ) -> Event<L> {
        debug_assert!(self.is_current(object));
        *self.object_mut(object.slot).counters.get_mut(kind) -= amount;
        if layered {
            self.flags.derived_dirty = true;
        }
        Event::CountersRemoved {
            object,
            kind,
            amount,
        }
    }

    /// Turns the face-down permanent in `slot` face up. It stays the same object, with its
    /// counters, damage and status, and does not enter again
    /// ([CR 708.8](https://yawgatog.com/resources/magic-rules/#R7088)): "As a face-down
    /// permanent is turned face up, its copiable values revert to its normal copiable values.
    /// Any effects that have been applied to the face-down permanent still apply to the
    /// face-up permanent. Any abilities relating to the permanent entering the battlefield
    /// don't trigger and don't have any effect, because the permanent has already entered the
    /// battlefield."
    ///
    /// Every player has seen it (CR 701.40b), and it gets a new timestamp
    /// ([CR 613.7f](https://yawgatog.com/resources/magic-rules/#R6137f)): "A permanent
    /// receives a new timestamp each time it turns face up or face down."
    #[inline]
    pub fn turn_face_up(&mut self, slot: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        let timestamp = self.zones.next_timestamp;
        self.zones.next_timestamp = timestamp.wrapping_add(1);
        let o = self.object_mut(slot);
        o.face = FaceIx::FRONT;
        o.timestamp = timestamp;
        for k in &mut self.knowledge {
            k.insert(slot);
        }
        self.flags.derived_dirty = true;
    }

    /// Makes the card just put on the stack the spell its caster chose: `face` up, under
    /// their control ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a)): "It
    /// has all the characteristics of the card (or the copy of a card) associated with it,
    /// and that player becomes its controller."
    #[inline]
    pub fn become_spell(&mut self, slot: L::Slot, face: FaceIx, controller: Seat) {
        debug_assert_eq!(self.object(slot).zone, Zone::Stack);
        let o = self.object_mut(slot);
        o.face = face;
        o.base_controller = controller;
    }

    /// Readies a free slot of the token pool to be a token with the copiable values `what`:
    /// a predefined token's ([`Copiable::of_card`]) or a copy's
    /// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072): "the copy acquires
    /// the copiable values of the original object's characteristics"), so never a hidden
    /// card's ([`Copiable::record`]).
    /// [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player who
    /// creates a token is its owner." The token exists once it is moved onto the
    /// battlefield, where it enters with the face returned up
    /// ([CR 707.8a](https://yawgatog.com/resources/magic-rules/#R7078a)): "If the token is a
    /// copy of a double-faced permanent with its back face up, the token enters the
    /// battlefield with its back face up."
    #[inline]
    #[must_use]
    pub fn make_token(
        &mut self,
        t: &GameTable,
        slot: L::Slot,
        owner: Seat,
        what: Copiable,
    ) -> FaceIx {
        let (card, face) = what.record(t);
        self.ready_token(slot, owner, card, what.is_one_one());
        face
    }

    /// [`State::make_token`] of the definition at `card`, `one_one` for a copy that is 1/1 by
    /// exception ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)).
    #[inline]
    pub(crate) fn ready_token(&mut self, slot: L::Slot, owner: Seat, card: CardIx, one_one: bool) {
        debug_assert_eq!(self.object(slot).zone, Zone::Nowhere);
        let o = self.object_mut(slot);
        o.card = card;
        o.owner = owner;
        o.base_controller = owner;
        o.flags = ObjectFlags(ObjectFlags::TOKEN);
        o.flags.set(ObjectFlags::ONE_ONE, one_one);
    }

    /// Gives the permanent that just entered in `slot` the status it enters with. Part of
    /// its zone change: no event of its own, and the move has already marked the layer
    /// results stale.
    #[inline]
    pub fn arrive(&mut self, slot: L::Slot, how: Arrival<L>) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        debug_assert!(self.flags.derived_dirty);
        let o = self.object_mut(slot);
        o.flags.set(ObjectFlags::TAPPED, how.tapped);
        o.flags.set(ObjectFlags::ATTACKING, how.attacking);
        o.face = how.face;
        if let Some(controller) = how.controller {
            o.base_controller = controller;
        }
        if let Some((kind, n)) = how.counters {
            debug_assert!(!Counters::is_long_tail(kind));
            *o.counters.get_mut(kind) = n;
        }
        if let Some(host) = how.attach {
            o.attached_to = Some(host);
        }
    }

    /// Records that the object in `source` exiled the card in `slot`
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a),
    /// [CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)). Which of the two links
    /// it is is a flag of milestone 4's ([`State::legacy_mut`]).
    #[inline]
    pub fn link(&mut self, slot: L::Slot, source: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Exile);
        self.object_mut(slot).linked = Some(source);
    }

    /// Makes a creature an attacking creature
    /// ([CR 508.1k](https://yawgatog.com/resources/magic-rules/#R5081k)): "Each chosen
    /// creature still controlled by the active player becomes an attacking creature." It
    /// attacks the defending player until [`State::choose_attacked`] says otherwise.
    #[inline]
    pub fn declare_attacker(&mut self, slot: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot)
            .flags
            .set(ObjectFlags::ATTACKING, true);
    }

    /// Has an attacking creature attack the planeswalker in `planeswalker`, or the defending
    /// player ([CR 506.2](https://yawgatog.com/resources/magic-rules/#R5062)): "that player,
    /// planeswalkers they control, and battles they protect may be attacked."
    #[inline]
    pub fn choose_attacked(&mut self, attacker: L::Slot, planeswalker: Option<L::Slot>) {
        debug_assert!(self.object(attacker).flags.has(ObjectFlags::ATTACKING));
        let o = self.object_mut(attacker);
        o.combat = planeswalker;
        o.flags
            .set(ObjectFlags::ATTACKS_PLANESWALKER, planeswalker.is_some());
    }

    /// Has an attacker whose planeswalker is no longer one its opponent controls attack
    /// nothing ([CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064)): "A
    /// planeswalker or battle that's removed from combat stops being attacked." The creature
    /// stays an attacking creature
    /// ([CR 506.4c](https://yawgatog.com/resources/magic-rules/#R5064c)).
    #[inline]
    pub fn forget_attacked(&mut self, attacker: L::Slot) {
        self.object_mut(attacker).combat = None;
    }

    /// Makes a creature a blocking creature, whose attacker [`State::block`] names next
    /// ([CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g)): "Each chosen
    /// creature still controlled by the defending player becomes a blocking creature."
    #[inline]
    pub fn declare_blocker(&mut self, blocker: L::Slot) {
        debug_assert_eq!(self.object(blocker).zone, Zone::Battlefield);
        self.object_mut(blocker)
            .flags
            .set(ObjectFlags::BLOCKING, true);
    }

    /// Has a blocking creature block `attacker`, which becomes blocked
    /// ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h)): "An attacking
    /// creature with one or more creatures declared as blockers for it becomes a blocked
    /// creature".
    #[inline]
    pub fn block(&mut self, blocker: L::Slot, attacker: L::Slot) {
        debug_assert!(self.object(blocker).flags.has(ObjectFlags::BLOCKING));
        debug_assert!(self.object(attacker).flags.has(ObjectFlags::ATTACKING));
        self.object_mut(blocker).combat = Some(attacker);
        self.object_mut(attacker)
            .flags
            .set(ObjectFlags::BLOCKED, true);
    }

    /// Records that a creature assigns its combat damage in the first-strike step, so it
    /// deals none in the second unless it has double strike
    /// ([CR 510.4](https://yawgatog.com/resources/magic-rules/#R5104)).
    #[inline]
    pub fn mark_struck_first(&mut self, slot: L::Slot) {
        self.object_mut(slot)
            .flags
            .set(ObjectFlags::STRUCK_FIRST, true);
    }

    /// Removes a permanent from combat
    /// ([CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064)): "A creature that's
    /// removed from combat stops being an attacking, blocking, blocked, and/or unblocked
    /// creature. A planeswalker or battle that's removed from combat stops being attacked."
    /// What attacked it or blocked it stays in combat, related to nothing.
    #[inline]
    pub fn remove_from_combat(&mut self, slot: L::Slot) {
        let o = self.object_mut(slot);
        o.flags.set(ObjectFlags::COMBAT, false);
        o.combat = None;
        for i in 0..self.battlefield.len() {
            let other = self.battlefield.as_slice()[i];
            if self.object(other).combat == Some(slot) {
                self.object_mut(other).combat = None;
            }
        }
    }

    /// Removes every permanent from combat as it ends
    /// ([CR 511.3](https://yawgatog.com/resources/magic-rules/#R5113)): "As soon as the end
    /// of combat step ends, all creatures, battles, and planeswalkers are removed from
    /// combat." The turn forgets that attackers were declared
    /// ([`State::declare_attack`]).
    #[inline]
    pub fn end_combat(&mut self) {
        self.turn.attacked = false;
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            let o = self.object_mut(slot);
            o.flags.set(ObjectFlags::COMBAT, false);
            o.combat = None;
        }
    }

    /// Ends the floating effects on objects that have left their zone: such an object no
    /// longer exists ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)). The
    /// queue is milestone 4's to replace (by-w8gv.7.1); [`State::rederive`] does this before
    /// it lends the cache.
    #[inline]
    pub(crate) fn drop_stale_effects(&mut self) {
        let objects = &self.objects;
        self.effects
            .retain(|e| objects[e.affected.slot.index()].incarnation == e.affected.incarnation);
    }
}
