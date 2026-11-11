//! The board door: arranging a position by hand, for scenarios and tests (architecture
//! §17.1) and for a saved game rebuilt from a turn-start checkpoint (§14): the turn and step,
//! where each card is, life totals and tapped permanents. None of this is a rules action, no
//! player decides anything and no event says it happened; the moves still go through the zone
//! chokepoints, so identities, knowledge and zone membership stay in agreement.
//!
//! [`State::board`] is the one way in (the crate's contract, "The board door"). Rules code
//! and card bodies never open it: a search for `.board()` finds whoever does.

use beryl_card::{FaceIx, Trigger};

use crate::event::Sink;
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat, Slot, SlotId};
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::{
    CounterKind, Counters, ManaPool, MulliganStatus, ObjectFlags, Pending, State, TurnStep, Zone,
};
use crate::table::GameTable;
use crate::two_player::TwoPlayerSeat as _;
use crate::zone::Place;

/// Lent by [`State::board`]: the handle that arranges a position with no event and no rules
/// action.
pub struct Board<'a, L: Layout>(&'a mut State<L>);

/// A way [`Board::corrupt`] breaks an object.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum Corruption {
    /// Says the object is in a zone it is not listed in.
    Zone(Zone),
    /// Taps it wherever it is.
    Tapped,
    /// Takes the top card off its owner's library list, leaving that card a member.
    Unlisted,
}

impl<L: Layout> State<L> {
    /// Opens the board door, for scenarios, checkpoints, tools and tests.
    #[inline]
    pub fn board(&mut self) -> Board<'_, L> {
        Board(self)
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
            let ability = &t.def(d.card).face(d.face).abilities[d.ability.index()];
            matches!(ability.trigger, Trigger::Delayed(_))
        }) {
            return Some(format!("a delayed trigger of {}", t.def(d.card).name));
        }
        if let Some(stored) = self.stores_lingering(t) {
            return Some(stored);
        }
        if let Some(p) = self.permissions.as_slice().first() {
            return Some(format!(
                "player {}'s permission to cast {}",
                p.player.get(),
                name(p.object.slot)
            ));
        }
        self.rule_change_lingering()
    }
}

impl<L: Layout> Board<'_, L> {
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
            matches!(self.0.pending, Pending::PlayOrDraw { .. }),
            "begin_at on a game already begun"
        );
        self.0.turn.number = turn;
        self.0.turn.active = active;
        self.0.turn.starting_player = if turn % 2 == 1 {
            active
        } else {
            active.opponent()
        };
        self.0.turn.step = step;
        self.0.turn.priority = active;
        self.0.turn.passes = 0;
        self.0.pending = Pending::None;
        for p in self.0.players.as_mut_slice() {
            p.mulligan = MulliganStatus::Kept;
        }
    }

    /// Skips the start of the game and puts it just before turn `turn` begins, with `active`
    /// its active player and both opening hands kept: the previous turn's cleanup step is
    /// over, so the engine's next call begins the turn, untapping, emptying what lasted
    /// "until your next turn" and triggering what the turn's beginning triggers, as play would
    /// ([CR 514.3](https://yawgatog.com/resources/magic-rules/#R5143)): "Normally, no player
    /// receives priority during the cleanup step". Call it on a new state, before any
    /// decision is answered. Turns alternate from the starting player
    /// ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038)).
    pub fn begin_before_turn(&mut self, turn: u16, active: Seat) {
        assert!(turn >= 1, "turns are numbered from 1");
        assert!(
            matches!(self.0.pending, Pending::PlayOrDraw { .. }),
            "begin_before_turn on a game already begun"
        );
        self.0.turn.starting_player = if turn % 2 == 1 {
            active
        } else {
            active.opponent()
        };
        self.0.turn.number = turn - 1;
        // The previous turn's active player, or before the first turn the starting player:
        // the turn begun next is theirs, or their opponent's.
        self.0.turn.active = if turn == 1 { active } else { active.opponent() };
        self.0.turn.step = TurnStep::Cleanup;
        self.0.turn.priority = self.0.turn.active;
        self.0.turn.passes = 0;
        self.0.turn.cleanup_check = true;
        self.0.pending = Pending::None;
        for p in self.0.players.as_mut_slice() {
            p.mulligan = MulliganStatus::Kept;
        }
    }

    /// Moves the object in `slot` to the top of `zone`. A permanent put onto the battlefield
    /// this way has been there since its controller's turn began, so it is not summoning sick
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub fn place(&mut self, slot: L::Slot, zone: Zone) -> Result<(), Full> {
        self.0
            .move_object(&mut Sink::off(), slot, zone, Place::Top)?;
        if zone == Zone::Battlefield {
            self.0
                .object_mut(slot)
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
        debug_assert_eq!(self.0.object(slot).zone, Zone::Exile);
        debug_assert_eq!(self.0.object(source).zone, Zone::Battlefield);
        self.0.link(slot, source);
        self.0
            .legacy_mut()
            .set_object_flags(slot, ObjectFlags::EXILED_WITH, true);
    }

    /// Links the exiled card in `slot` to the permanent in `source`, which returns it when it
    /// leaves the battlefield
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot effects
    /// cause an object to change zones 'until' a specified event occurs."
    pub fn set_exiled_until(&mut self, slot: L::Slot, source: L::Slot) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Exile);
        debug_assert_eq!(self.0.object(source).zone, Zone::Battlefield);
        self.0.link(slot, source);
        self.0
            .legacy_mut()
            .set_object_flags(slot, ObjectFlags::EXILED_UNTIL, true);
    }

    /// Creates a token of the game-table card `card` on the battlefield, owned and controlled
    /// by `owner` ([CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player
    /// who creates a token is its owner"), with face `face` up and, for a copy that is 1/1 by
    /// exception, `one_one` ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)).
    /// No ability triggers: it is already there. Like [`Board::place`], it has been there since
    /// its controller's turn began.
    pub fn place_token(
        &mut self,
        owner: Seat,
        card: L::Card,
        face: FaceIx,
        one_one: bool,
    ) -> Result<L::Slot, Full> {
        let slot = L::Slot::pool()
            .find(|&slot| self.0.object(slot).zone == Zone::Nowhere)
            .ok_or(Full)?;
        self.0.ready_token(slot, owner, card, one_one);
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
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0.object_mut(slot).base_controller = player;
        self.0.flags.derived_dirty = true;
    }

    /// Whether the permanent in `slot` has been under its controller's control continuously
    /// since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub fn set_sick(&mut self, slot: L::Slot, sick: bool) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0
            .object_mut(slot)
            .flags
            .set(ObjectFlags::SUMMONING_SICK, sick);
    }

    /// Attaches the Aura or Equipment in `slot` to the permanent in `to`
    /// ([CR 701.3a](https://yawgatog.com/resources/magic-rules/#R7013a)).
    pub fn attach(&mut self, slot: L::Slot, to: L::Slot) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        debug_assert_eq!(self.0.object(to).zone, Zone::Battlefield);
        self.0.object_mut(slot).attached_to = Some(to);
        self.0.flags.derived_dirty = true;
    }

    /// Marks `damage` on the permanent in `slot`
    /// ([CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e)).
    pub fn set_damage(&mut self, slot: L::Slot, damage: u8) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0.object_mut(slot).damage = damage;
    }

    /// Whether the permanent in `slot` was cast for its impending cost
    /// ([CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)).
    pub fn set_impending(&mut self, slot: L::Slot, impending: bool) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0
            .legacy_mut()
            .set_object_flags(slot, ObjectFlags::IMPENDING, impending);
    }

    /// Gives `player` the emblem of the game-table card `card`
    /// ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)).
    pub fn add_emblem(&mut self, player: Seat, card: L::Card) -> Result<(), Full> {
        self.0.add_emblem(player, card)
    }

    /// Whether `player` knows the identity of the object in `slot` (architecture §13), as a
    /// look or a reveal would have told them.
    pub fn set_known(&mut self, player: Seat, slot: L::Slot, known: bool) {
        let k = &mut self.0.knowledge[player.index()];
        if known {
            k.insert(slot);
        } else {
            k.remove(slot);
        }
    }

    /// Sets how many counters of a kind a permanent has, such as a planeswalker's loyalty.
    pub fn set_counters(&mut self, slot: L::Slot, kind: CounterKind, n: u8) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        if Counters::is_long_tail(kind) {
            self.0.legacy_mut().set_counters(slot, kind, n);
        } else {
            *self.0.object_mut(slot).counters.get_mut(kind) = n;
        }
    }

    /// Turns a double-faced permanent so that the face at `face` is up, as one that entered
    /// transformed has its back face up
    /// ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)), or turns a
    /// permanent face down ([`FaceIx::DOWN`]), as a manifested one is. Then only its
    /// controller knows it (CR 708.5).
    pub fn set_face(&mut self, slot: L::Slot, face: FaceIx) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0.object_mut(slot).face = face;
        if face == FaceIx::DOWN {
            let controller = self.0.object(slot).base_controller;
            for other in self.0.players().without(controller).in_seat_order() {
                self.0.knowledge[other.index()].remove(slot);
            }
        }
        self.0.flags.derived_dirty = true;
    }

    /// Taps or untaps a permanent.
    pub fn set_tapped(&mut self, slot: L::Slot, tapped: bool) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0
            .object_mut(slot)
            .flags
            .set(ObjectFlags::TAPPED, tapped);
    }

    /// Whether the permanent in `slot` entered the battlefield this turn.
    pub fn set_entered_this_turn(&mut self, slot: L::Slot, entered: bool) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0
            .object_mut(slot)
            .flags
            .set(ObjectFlags::ENTERED_THIS_TURN, entered);
    }

    /// Whether a loyalty ability of the permanent in `slot` was activated this turn
    /// ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)).
    pub fn set_loyalty_activated(&mut self, slot: L::Slot, activated: bool) {
        debug_assert_eq!(self.0.object(slot).zone, Zone::Battlefield);
        self.0
            .legacy_mut()
            .set_object_flags(slot, ObjectFlags::LOYALTY_ACTIVATED, activated);
    }

    /// Trades the identities of two cards, as the deal could have dealt them: which card a
    /// slot holds says nothing until a player knows it (architecture §13).
    pub fn swap_cards(&mut self, a: L::Slot, b: L::Slot) {
        let (card_a, card_b) = (self.0.object(a).card, self.0.object(b).card);
        self.0.object_mut(a).card = card_b;
        self.0.object_mut(b).card = card_a;
    }

    /// Breaks the state on purpose, for the tests of the invariant check
    /// ([`State::check_invariants`]).
    #[doc(hidden)]
    pub fn corrupt(&mut self, slot: L::Slot, how: Corruption) {
        let owner = self.0.object(slot).owner.index();
        match how {
            Corruption::Zone(zone) => self.0.object_mut(slot).zone = zone,
            Corruption::Tapped => {
                self.0.object_mut(slot).flags.set(ObjectFlags::TAPPED, true);
            }
            Corruption::Unlisted => {
                self.0.zones.library[owner].pop();
            }
        }
    }

    pub fn set_life(&mut self, player: Seat, life: i16) {
        self.0.player_mut(player).life = life;
    }

    /// Puts `pool` in a player's mana pool as its unspent mana (CR 106.4).
    pub fn set_mana(&mut self, player: Seat, pool: ManaPool) {
        self.0.player_mut(player).mana = pool;
    }

    /// Says a player has played `lands` lands this turn (CR 305.2).
    pub fn set_lands_played(&mut self, player: Seat, lands: u8) {
        self.0.player_mut(player).lands_played = lands;
    }

    /// Makes the turn the game is in `active`'s, its number and step as they are.
    pub fn set_active(&mut self, active: Seat) {
        self.0.turn.active = active;
    }

    /// Puts the game in `step` of the turn it is in, performing none of the step's
    /// turn-based actions and leaving priority where it is.
    pub fn set_step(&mut self, step: TurnStep) {
        self.0.turn.step = step;
    }

    /// Hands priority to a player, no player having passed since.
    pub fn set_priority(&mut self, player: Seat) {
        self.0.give_priority(player);
    }

    /// Puts a player's library in the given order, top first. `top_first` must hold exactly
    /// the slots of that library.
    pub fn order_library(&mut self, player: Seat, top_first: &[L::Slot]) {
        let library = &mut self.0.zones.library[player.index()];
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

    /// Settles who won the coin flip of [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)
    /// as a record says, rather than as the game's randomness drew it: "the players determine
    /// which one of them will choose who takes the first turn". Only while the winner's choice
    /// to play or draw is open.
    ///
    /// # Panics
    ///
    /// If the choice to play or draw is not open.
    pub fn set_flip_winner(&mut self, chooser: Seat) {
        assert!(
            matches!(self.0.pending, Pending::PlayOrDraw { .. }),
            "the choice to play or draw is open"
        );
        self.0.pending = Pending::PlayOrDraw { chooser };
    }
}

impl Board<'_, Small> {
    /// Renumbers each library's slots last first ([`Board::renumber_deck_slots`]). For tests.
    ///
    /// # Panics
    ///
    /// If the game has begun: the choice to play or draw is not open.
    #[doc(hidden)]
    pub fn reverse_deck_slots(&mut self) {
        self.renumber_deck_slots(<[Slot]>::reverse);
    }

    /// Renumbers each library's slots before the game begins: `order` rearranges the slots of
    /// a library, and the card at each place in it moves to the slot now there. Each card
    /// stays where [`State::new`] put it in its library, and the random streams are untouched,
    /// so the game plays out the same under other slot numbers. Slot numbers are public
    /// handles that say nothing about a card (architecture §13), so a save must replay the
    /// same either way (§14), and so must a scenario. For tests.
    ///
    /// # Panics
    ///
    /// If the game has begun: the choice to play or draw is not open.
    #[doc(hidden)]
    pub fn renumber_deck_slots(&mut self, mut order: impl FnMut(&mut [Slot])) {
        assert!(
            matches!(self.0.pending, Pending::PlayOrDraw { .. }) && self.0.turn.number == 0,
            "the game has not begun"
        );
        for player in Seat::BOTH {
            let p = player.index();
            let mut slots = self.0.zones.library[p];
            let mut cards: <Small as Layout>::Deck<CardIx> = Fixed::new();
            for &slot in &slots {
                let _ = cards.push(self.0.object(slot).card);
            }
            order(slots.as_mut_slice());
            for (&slot, &card) in slots.iter().zip(cards.iter()) {
                self.0.object_mut(slot).card = card;
            }
            self.0.zones.library[p] = slots;
        }
    }
}
