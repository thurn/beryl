//! Arranging a position by hand, for scenarios and tests (architecture §17.1) and for a saved
//! game rebuilt from a turn-start checkpoint (§14): the turn and step, where each card is,
//! life totals and tapped permanents. None of this is a rules action, and no player decides
//! anything; the moves still go through the zone chokepoints, so identities, knowledge and
//! zone membership stay in agreement.

use crate::card::{FaceIx, Trigger};
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::{CardIx, Seat};
use crate::layout::{Layout, Queue};
use crate::state::{
    CounterKind, Duration, MulliganStatus, ObjectFlags, Pending, State, TurnStep, Zone, token_slots,
};
use crate::table::GameTable;
use crate::{trigger, zone};

impl<L: Layout> State<L> {
    /// Skips the start of the game and puts it at `step` of turn `turn`, with `active` the
    /// active player and holding priority, and both opening hands kept. The step's turn-based
    /// actions are not performed: the game is already in it. Call it on a new state, before
    /// any decision is answered.
    ///
    /// Turns alternate from the starting player
    /// ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038)), so `active` takes the
    /// odd-numbered turns.
    ///
    /// The step must be one where players get priority:
    /// [CR 502.4](https://yawgatog.com/resources/magic-rules/#R5024): "No player receives
    /// priority during the untap step", and
    /// [CR 514.3](https://yawgatog.com/resources/magic-rules/#R5143): "Normally, no player
    /// receives priority during the cleanup step".
    pub fn begin_at(&mut self, turn: u16, active: Seat, step: TurnStep) {
        assert!(turn >= 1, "turns are numbered from 1");
        assert!(
            !matches!(step, TurnStep::Untap | TurnStep::Cleanup),
            "no priority in the {step:?} step"
        );
        assert!(
            matches!(self.pending, Pending::PlayOrDraw { .. }),
            "begin_at on a game already begun"
        );
        self.turn.number = turn;
        self.turn.active = active;
        self.turn.starting_player = if turn % 2 == 1 {
            active
        } else {
            active.opponent()
        };
        self.turn.step = step;
        self.turn.priority = active;
        self.turn.passes = 0;
        self.pending = Pending::None;
        for p in &mut self.players {
            p.mulligan = MulliganStatus::Kept;
        }
    }

    /// Skips the start of the game and puts it just before turn `turn` begins, with `active`
    /// its active player and both opening hands kept: the previous turn's cleanup step is
    /// over, so the engine's next call begins the turn, untapping, emptying what lasted
    /// "until your next turn" and triggering what the turn's beginning triggers, as play would
    /// ([CR 514.3a](https://yawgatog.com/resources/magic-rules/#R5143a)): "If no state-based
    /// actions are performed and no triggered abilities trigger during the cleanup step, no
    /// player receives priority and the turn ends." Call it on a new state, before any
    /// decision is answered. Turns alternate from the starting player
    /// ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038)).
    pub fn begin_before_turn(&mut self, turn: u16, active: Seat) {
        assert!(turn >= 1, "turns are numbered from 1");
        assert!(
            matches!(self.pending, Pending::PlayOrDraw { .. }),
            "begin_before_turn on a game already begun"
        );
        self.turn.starting_player = if turn % 2 == 1 {
            active
        } else {
            active.opponent()
        };
        self.turn.number = turn - 1;
        // The previous turn's active player, or before the first turn the starting player:
        // the turn begun next is theirs, or their opponent's.
        self.turn.active = if turn == 1 { active } else { active.opponent() };
        self.turn.step = TurnStep::Cleanup;
        self.turn.priority = self.turn.active;
        self.turn.passes = 0;
        self.turn.cleanup_check = true;
        self.pending = Pending::None;
        for p in &mut self.players {
            p.mulligan = MulliganStatus::Kept;
        }
    }

    /// What of the game, beyond what a turn-start checkpoint records of its board, rebuilding
    /// from that board before this turn began would lose, in words, if anything: an effect
    /// lasting past the turn's beginning, a delayed trigger waiting, a permission to cast. Call
    /// it as a turn begins, once the active player's permanents have untapped. The abilities
    /// the turn's beginning triggered are no loss: beginning the turn again triggers them
    /// again. Nothing else on [`State`] outlasts a turn: the stack is empty between turns,
    /// combat and per-turn counts end with the turn, and summoning sickness is on the board.
    pub fn off_board(&self, t: &GameTable) -> Option<String> {
        let name = |slot: L::Slot| t.def(self.card(slot)).name;
        if self.turn.step != TurnStep::Upkeep {
            return Some(format!("the {:?} step, not the upkeep", self.turn.step));
        }
        if self.pending != Pending::None {
            return Some("a question open".to_owned());
        }
        if let Some(entry) = self.stack.as_slice().first() {
            return Some(format!("{} on the stack", t.def(entry.card).name));
        }
        if let Some(d) = self.delayed.as_slice().first() {
            return Some(format!("a delayed trigger of {}", t.def(d.card).name));
        }
        if let Some(d) = self.triggers.as_slice().iter().find(|d| {
            matches!(
                trigger::ability(t, d.card, d.face, d.ability).trigger,
                Trigger::Delayed(_)
            )
        }) {
            return Some(format!("a delayed trigger of {}", t.def(d.card).name));
        }
        if let Some(e) = self.effects.as_slice().first() {
            let until = match e.duration {
                Duration::EndOfTurn => "until end of turn".to_owned(),
                Duration::UntilYourNextTurn(p) => format!("until player {}'s next turn", p.get()),
                Duration::Indefinite => "indefinitely".to_owned(),
            };
            return Some(format!(
                "an effect of {} on {} lasting {until}",
                t.def(e.source).name,
                name(e.affected.slot)
            ));
        }
        if let Some(p) = self.permissions.as_slice().first() {
            return Some(format!(
                "player {}'s permission to cast {}",
                p.player.get(),
                name(p.object.slot)
            ));
        }
        for p in Seat::BOTH {
            if self.player(p).sorceries_as_flash {
                return Some(format!(
                    "player {} casting sorceries as though they had flash until their next turn",
                    p.get()
                ));
            }
        }
        None
    }

    /// Moves the object in `slot` to the top of `zone`. A permanent put onto the battlefield
    /// this way has been there since its controller's turn began, so it is not summoning sick
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub fn place(&mut self, slot: L::Slot, zone: Zone) -> Result<(), Full> {
        zone::move_to(self, &mut Sink::off(), slot, zone)?;
        if zone == Zone::Battlefield {
            self.object_mut(slot)
                .flags
                .set(ObjectFlags::SUMMONING_SICK, false);
        }
        Ok(())
    }

    /// Links the exiled card in `slot` to the permanent in `source` as one of the cards
    /// "exiled with" it, as an ability of `source` exiling it would have
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "If an object has an
    /// activated or triggered ability printed on it that instructs a player to exile one or more
    /// cards and an ability printed on it that refers either to 'the exiled cards' or to cards
    /// 'exiled with [this object],' these abilities are linked."
    pub fn set_exiled_with(&mut self, slot: L::Slot, source: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Exile);
        debug_assert_eq!(self.object(source).zone, Zone::Battlefield);
        let o = self.object_mut(slot);
        o.flags.set(ObjectFlags::EXILED_WITH, true);
        o.linked = Some(source);
    }

    /// Links the exiled card in `slot` to the permanent in `source`, which returns it when it
    /// leaves the battlefield
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot effects
    /// cause an object to change zones 'until' a specified event occurs."
    pub fn set_exiled_until(&mut self, slot: L::Slot, source: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Exile);
        debug_assert_eq!(self.object(source).zone, Zone::Battlefield);
        let o = self.object_mut(slot);
        o.flags.set(ObjectFlags::EXILED_UNTIL, true);
        o.linked = Some(source);
    }

    /// Creates a token of the game-table card `card` on the battlefield, owned and controlled
    /// by `owner` ([CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player
    /// who creates a token is its owner"), with face `face` up and, for a copy that is 1/1 by
    /// exception, `one_one` ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)).
    /// No ability triggers: it is already there. Like [`State::place`], it has been there since
    /// its controller's turn began.
    pub fn place_token(
        &mut self,
        owner: Seat,
        card: CardIx,
        face: FaceIx,
        one_one: bool,
    ) -> Result<L::Slot, Full> {
        let slot = token_slots()
            .find(|&slot: &L::Slot| self.object(slot).zone == Zone::Nowhere)
            .ok_or(Full)?;
        let o = self.object_mut(slot);
        o.card = card;
        o.owner = owner;
        o.base_controller = owner;
        o.flags = ObjectFlags(ObjectFlags::TOKEN);
        o.flags.set(ObjectFlags::ONE_ONE, one_one);
        self.place(slot, Zone::Battlefield)?;
        if face != FaceIx::FRONT {
            self.set_face(slot, face);
        }
        Ok(slot)
    }

    /// Makes `player` the controller of the permanent in `slot` before continuous effects, as
    /// if it had entered the battlefield under their control
    /// ([CR 110.2](https://yawgatog.com/resources/magic-rules/#R1102)): "A permanent's
    /// controller is, by default, the player under whose control it entered the battlefield."
    pub fn set_controller(&mut self, slot: L::Slot, player: Seat) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot).base_controller = player;
        self.flags.derived_dirty = true;
    }

    /// Whether the permanent in `slot` has been under its controller's control continuously
    /// since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub fn set_sick(&mut self, slot: L::Slot, sick: bool) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot)
            .flags
            .set(ObjectFlags::SUMMONING_SICK, sick);
    }

    /// Attaches the Aura or Equipment in `slot` to the permanent in `to`
    /// ([CR 701.3a](https://yawgatog.com/resources/magic-rules/#R7013a)).
    pub fn attach(&mut self, slot: L::Slot, to: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        debug_assert_eq!(self.object(to).zone, Zone::Battlefield);
        self.object_mut(slot).attached_to = Some(to);
        self.flags.derived_dirty = true;
    }

    /// Marks `damage` on the permanent in `slot`
    /// ([CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e)).
    pub fn set_damage(&mut self, slot: L::Slot, damage: u8) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot).damage = damage;
    }

    /// Whether the permanent in `slot` was cast for its impending cost
    /// ([CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)).
    pub fn set_impending(&mut self, slot: L::Slot, impending: bool) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot)
            .flags
            .set(ObjectFlags::IMPENDING, impending);
        self.flags.derived_dirty = true;
    }

    /// Gives `player` the emblem of the game-table card `card`
    /// ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)).
    pub fn add_emblem(&mut self, player: Seat, card: CardIx) -> Result<(), Full> {
        self.player_mut(player).emblems.push(card)
    }

    /// Whether `player` knows the identity of the object in `slot` (architecture §13), as a
    /// look or a reveal would have told them.
    pub fn set_known(&mut self, player: Seat, slot: L::Slot, known: bool) {
        let k = &mut self.knowledge[player.index()];
        if known {
            k.insert(slot);
        } else {
            k.remove(slot);
        }
    }

    /// Sets how many counters of a kind a permanent has, such as a planeswalker's loyalty.
    pub fn set_counters(&mut self, slot: L::Slot, kind: CounterKind, n: u8) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        *self.object_mut(slot).counters.get_mut(kind) = n;
    }

    /// Turns a double-faced permanent so that the face at `face` is up, as one that entered
    /// transformed has its back face up
    /// ([CR 712.14a](https://yawgatog.com/resources/magic-rules/#R71214a)), or turns a
    /// permanent face down ([`FaceIx::DOWN`]), as a manifested one is. Then only its
    /// controller knows it (CR 708.5).
    pub fn set_face(&mut self, slot: L::Slot, face: FaceIx) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot).face = face;
        if face == FaceIx::DOWN {
            let other = self.object(slot).base_controller.opponent();
            self.knowledge[other.index()].remove(slot);
        }
        self.flags.derived_dirty = true;
    }

    /// Taps or untaps a permanent.
    pub fn set_tapped(&mut self, slot: L::Slot, tapped: bool) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        self.object_mut(slot).flags.set(ObjectFlags::TAPPED, tapped);
    }

    pub fn set_life(&mut self, player: Seat, life: i16) {
        self.player_mut(player).life = life;
    }

    /// Puts a player's library in the given order, top first. `top_first` must hold exactly
    /// the slots of that library.
    pub fn order_library(&mut self, player: Seat, top_first: &[L::Slot]) {
        let library = &mut self.zones.library[player.index()];
        assert_eq!(library.len(), top_first.len(), "not the whole library");
        let distinct: crate::bitboard::Bitboard<L::Slot> = top_first.iter().copied().collect();
        assert_eq!(distinct.count() as usize, top_first.len(), "a slot twice");
        for &slot in top_first {
            assert!(
                library.position(slot).is_some(),
                "slot {slot} is not in the library"
            );
        }
        for (i, &slot) in top_first.iter().rev().enumerate() {
            library.as_mut_slice()[i] = slot;
        }
    }
}
