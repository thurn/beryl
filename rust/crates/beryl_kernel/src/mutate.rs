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
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Table};
use crate::object::{AttackTarget, CombatRole, Flag, ObjectFlags, ObjectRef, SlotSet};
use crate::query::Copiable;
use crate::state::State;
use crate::table::GameTable;

/// How a permanent enters the battlefield, as the rules decided once its replacement effects
/// applied ([CR 614.12]): "Some replacement effects modify how a permanent enters the battlefield."
/// A record: the rules build one and [`State::arrive`] applies it.
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrival<L: Layout> {
    pub tapped: bool,
    /// Put onto the battlefield attacking this player ([CR 508.4]).
    ///
    /// [CR 508.4]: https://yawgatog.com/resources/magic-rules/#R5084
    pub attacking: Option<Seat>,
    /// Under this player's control rather than its owner's.
    pub controller: Option<Seat>,
    /// The face up.
    pub face: FaceIx,
    /// Counters it enters with ([CR 122.6]).
    ///
    /// [CR 122.6]: https://yawgatog.com/resources/magic-rules/#R1226
    pub counters: Option<(CounterKind, u8)>,
    /// The object an Aura attaches to as it enters ([CR 608.3c]).
    ///
    /// [CR 608.3c]: https://yawgatog.com/resources/magic-rules/#R6083c
    pub attach: Option<L::Slot>,
}

impl<L: Layout> State<L> {
    /// Taps or untaps a permanent ([CR 701.26a]): "To tap a permanent, turn it sideways from an
    /// upright position."
    ///
    /// [CR 701.26a]: https://yawgatog.com/resources/magic-rules/#R70126a
    #[inline]
    #[must_use]
    pub fn set_tapped(&mut self, object: ObjectRef<L>, tapped: bool) -> Event<L> {
        debug_assert!(self.is_current(object));
        self.object_mut(object.slot).flags.set(Flag::Tapped, tapped);
        if tapped {
            Event::Tap { object }
        } else {
            Event::Untap { object }
        }
    }

    /// Whether a permanent has not been under its controller's control continuously since their
    /// most recent turn began, as one a player just gained control of has not ([CR 302.6]).
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    #[inline]
    pub fn set_summoning_sick(&mut self, slot: L::Slot, sick: bool) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot).flags.set(Flag::SummoningSick, sick);
    }

    /// The per-turn reset of every permanent as `active`'s turn begins: theirs have now been under
    /// their control since their most recent turn began ([CR 302.6]). Reads each permanent's
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
    /// [CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c): "Damage dealt to a
    /// planeswalker causes that many loyalty counters to be removed from that planeswalker."
    ///
    /// [CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e): "Damage dealt to a creature
    /// by a source with neither wither nor infect causes that much damage to be marked on that
    /// creature."
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

    /// Removes the damage marked on every permanent, as the cleanup step does ([CR 514.2]): "all
    /// damage marked on permanents (including phased-out permanents) is removed".
    ///
    /// [CR 514.2]: https://yawgatog.com/resources/magic-rules/#R5142
    #[inline]
    pub fn clear_damage(&mut self) {
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            self.object_mut(slot).damage = 0;
        }
    }

    /// Removes the damage marked on the permanent `object`, as it regenerates ([CR 120.6]):
    /// "All damage marked on a permanent is removed when it regenerates".
    ///
    /// [CR 120.6]: https://yawgatog.com/resources/magic-rules/#R1206
    #[inline]
    pub fn remove_damage(&mut self, object: ObjectRef<L>) {
        debug_assert!(self.is_current(object));
        self.object_mut(object.slot).damage = 0;
    }

    /// Turns the face-down permanent in `slot` face up. It stays the same object, with its
    /// counters, damage and status, and does not enter again ([CR 708.8]): "As a face-down
    /// permanent is turned face up, its copiable values revert to its normal copiable values. Any
    /// effects that have been applied to the face-down permanent still apply to the face-up
    /// permanent. Any abilities relating to the permanent entering the battlefield don't trigger
    /// and don't have any effect, because the permanent has already entered the battlefield."
    ///
    /// Every player has seen it (CR 701.40b), and it gets a new timestamp ([CR 613.7f]): "A
    /// permanent receives a new timestamp each time it turns face up or face down."
    ///
    /// [CR 708.8]: https://yawgatog.com/resources/magic-rules/#R7088
    /// [CR 613.7f]: https://yawgatog.com/resources/magic-rules/#R6137f
    #[inline]
    pub fn turn_face_up(&mut self, slot: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        let timestamp = self.zones.next_timestamp;
        self.zones.next_timestamp = timestamp.wrapping_add(1);
        let o = self.object_mut(slot);
        o.face = FaceIx::FRONT;
        o.timestamp = timestamp;
        for k in self.knowledge.as_mut_slice() {
            k.insert(slot);
        }
        self.flags.derived_dirty = true;
    }

    /// Attaches the Equipment in `slot` to the permanent in `host`, both on the battlefield
    /// ([CR 701.3a]): "To attach an Aura, Equipment, or Fortification to an object or player means
    /// to take it from where it currently is and put it onto that object or player." The rules
    /// decide that it may.
    ///
    /// It gets a new timestamp ([CR 701.3c]): "Attaching an Aura, Equipment, or Fortification on
    /// the battlefield to a different object or player causes the Aura, Equipment, or Fortification
    /// to receive a new timestamp."
    ///
    /// [CR 701.3a]: https://yawgatog.com/resources/magic-rules/#R7013a
    /// [CR 701.3c]: https://yawgatog.com/resources/magic-rules/#R7013c
    #[inline]
    pub fn attach(&mut self, slot: L::Slot, host: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        debug_assert_eq!(self.object(host).zone, Zone::Battlefield);
        let timestamp = self.zones.next_timestamp;
        self.zones.next_timestamp = timestamp.wrapping_add(1);
        let o = self.object_mut(slot);
        o.attached_to = Some(host);
        o.timestamp = timestamp;
        self.flags.derived_dirty = true;
    }

    /// Unattaches the permanent in `slot` from what it is attached to, leaving it on the
    /// battlefield ([CR 704.5n]): "If an Equipment or Fortification is attached to an illegal
    /// permanent or to a player, it becomes unattached from that permanent or player. It remains on
    /// the battlefield."
    ///
    /// [CR 704.5n]: https://yawgatog.com/resources/magic-rules/#R7045n
    #[inline]
    pub fn unattach(&mut self, slot: L::Slot) {
        self.object_mut(slot).attached_to = None;
        self.flags.derived_dirty = true;
    }

    /// Makes the card just put on the stack the spell its caster chose: `face` up, under their
    /// control ([CR 601.2a]): "It has all the characteristics of the card (or the copy of a card)
    /// associated with it, and that player becomes its controller."
    ///
    /// [CR 601.2a]: https://yawgatog.com/resources/magic-rules/#R6012a
    #[inline]
    pub fn become_spell(&mut self, slot: L::Slot, face: FaceIx, controller: Seat) {
        debug_assert_eq!(self.object(slot).zone, Zone::Stack);
        let o = self.object_mut(slot);
        o.face = face;
        o.base_controller = controller;
    }

    /// Readies a free slot of the token pool to be a token with the copiable values `what`: a
    /// predefined token's ([`Copiable::of_card`]) or a copy's ([CR 707.2]: "the copy acquires the
    /// copiable values of the original object's characteristics"), so never a hidden card's
    /// ([`Copiable::record`]). [CR 111.2]: "The player who creates a token is its owner." The token
    /// exists once it is moved onto the battlefield, where it enters with the face returned up
    /// ([CR 707.8a]): "If the token is a copy of a double-faced permanent with its back face up,
    /// the token enters the battlefield with its back face up."
    ///
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    /// [CR 111.2]: https://yawgatog.com/resources/magic-rules/#R1112
    /// [CR 707.8a]: https://yawgatog.com/resources/magic-rules/#R7078a
    #[inline]
    #[must_use]
    pub fn make_token(
        &mut self,
        t: &GameTable,
        slot: L::Slot,
        owner: Seat,
        what: Copiable<L>,
    ) -> FaceIx {
        let (card, face) = what.record(t);
        self.ready_token(slot, owner, card, what.is_one_one());
        face
    }

    /// [`State::make_token`] of the definition at `card`, `one_one` for a copy that is 1/1 by
    /// exception ([CR 707.9b]).
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    #[inline]
    pub(crate) fn ready_token(&mut self, slot: L::Slot, owner: Seat, card: L::Card, one_one: bool) {
        debug_assert_eq!(self.object(slot).zone, Zone::Nowhere);
        let o = self.object_mut(slot);
        o.card = card;
        o.owner = owner;
        o.base_controller = owner;
        o.flags = ObjectFlags::NONE;
        o.flags.set(Flag::Token, true);
        o.flags.set(Flag::OneOne, one_one);
    }

    /// Gives the permanent that just entered in `slot` the status it enters with. Part of
    /// its zone change: no event of its own, and the move has already marked the layer
    /// results stale. [`Full`] if it enters with counters of a kind the table has no room
    /// for a row of ([`crate::counters`]).
    #[inline]
    pub fn arrive(&mut self, slot: L::Slot, how: Arrival<L>) -> Result<(), Full> {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
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
            o.attached_to = Some(host);
        }
        match how.counters {
            Some((kind, n)) => self.set_counters(slot, kind, n),
            None => Ok(()),
        }
    }

    /// Makes a creature an attacking creature that attacks `player` ([CR 508.1k]): "Each chosen
    /// creature still controlled by the active player becomes an attacking creature." It attacks
    /// that player until [`State::choose_attacked`] says otherwise.
    ///
    /// [CR 508.1k]: https://yawgatog.com/resources/magic-rules/#R5081k
    #[inline]
    pub fn declare_attacker(&mut self, slot: L::Slot, player: Seat) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot).combat = CombatRole::attacking(AttackTarget::Player(player));
    }

    /// Has an attacking creature attack `target` ([CR 506.2]): "that player, planeswalkers they
    /// control, and battles they protect may be attacked."
    ///
    /// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
    #[inline]
    pub fn choose_attacked(&mut self, attacker: L::Slot, target: AttackTarget<L>) {
        let CombatRole::Attacking {
            target: current, ..
        } = &mut self.object_mut(attacker).combat
        else {
            unreachable!("an attack target for a creature that is not attacking");
        };
        *current = target;
    }

    /// Has an attacker whose planeswalker is no longer one its opponent controls attack nothing
    /// ([CR 506.4]): "A planeswalker or battle that's removed from combat stops being attacked."
    /// The creature stays an attacking creature ([CR 506.4c]).
    ///
    /// [CR 506.4]: https://yawgatog.com/resources/magic-rules/#R5064
    /// [CR 506.4c]: https://yawgatog.com/resources/magic-rules/#R5064c
    #[inline]
    pub fn forget_attacked(&mut self, attacker: L::Slot) {
        self.choose_attacked(attacker, AttackTarget::Removed);
    }

    /// Makes a creature a blocking creature, whose attacker [`State::block`] names next
    /// ([CR 509.1g]): "Each chosen creature still controlled by the defending player becomes a
    /// blocking creature."
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn declare_blocker(&mut self, blocker: L::Slot) {
        debug_assert_eq!(self.object(blocker).zone, Zone::Battlefield);
        self.object_mut(blocker).combat = CombatRole::Blocking {
            attackers: SlotSet::EMPTY,
            struck_first: false,
        };
    }

    /// Has a blocking creature block `attacker`, which becomes blocked ([CR 509.1h]): "An attacking
    /// creature with one or more creatures declared as blockers for it becomes a blocked creature".
    /// [`Full`] if the blocker already blocks as many attackers as a [`SlotSet`] holds.
    ///
    /// [CR 509.1h]: https://yawgatog.com/resources/magic-rules/#R5091h
    #[inline]
    pub fn block(&mut self, blocker: L::Slot, attacker: L::Slot) -> Result<(), Full> {
        let CombatRole::Blocking { attackers, .. } = &mut self.object_mut(blocker).combat else {
            unreachable!("a creature blocking that was not declared as a blocker");
        };
        attackers.insert(attacker)?;
        let CombatRole::Attacking { blocked, .. } = &mut self.object_mut(attacker).combat else {
            unreachable!("a creature blocked that is not attacking");
        };
        *blocked = true;
        Ok(())
    }

    /// Records that a creature assigns its combat damage in the first-strike step, so it deals none
    /// in the second unless it has double strike ([CR 510.4]).
    ///
    /// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
    #[inline]
    pub fn mark_struck_first(&mut self, slot: L::Slot) {
        match &mut self.object_mut(slot).combat {
            CombatRole::Attacking { struck_first, .. }
            | CombatRole::Blocking { struck_first, .. } => {
                *struck_first = true;
            }
            CombatRole::None => unreachable!("a creature striking first that is not in combat"),
        }
    }

    /// Removes a permanent from combat ([CR 506.4]): "A creature that's removed from combat stops
    /// being an attacking, blocking, blocked, and/or unblocked creature. A planeswalker or battle
    /// that's removed from combat stops being attacked." What attacked it or blocked it stays in
    /// combat, related to nothing.
    ///
    /// [CR 506.4]: https://yawgatog.com/resources/magic-rules/#R5064
    #[inline]
    pub fn remove_from_combat(&mut self, slot: L::Slot) {
        self.object_mut(slot).combat = CombatRole::None;
        for i in 0..self.battlefield.len() {
            let other = self.battlefield.as_slice()[i];
            self.object_mut(other).combat.forget(slot);
        }
    }

    /// Removes every permanent from combat as it ends ([CR 511.3]): "As soon as the end of combat
    /// step ends, all creatures, battles, and planeswalkers are removed from combat." The turn
    /// forgets that attackers were declared ([`State::declare_attack`]).
    ///
    /// [CR 511.3]: https://yawgatog.com/resources/magic-rules/#R5113
    #[inline]
    pub fn end_combat(&mut self) {
        self.turn.attacked = false;
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            self.object_mut(slot).combat = CombatRole::None;
        }
    }
}
