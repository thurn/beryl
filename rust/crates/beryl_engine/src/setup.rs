//! Arranging a position by hand, for scenarios and tests (architecture §17.1): the turn and
//! step, where each card is, life totals and tapped permanents. None of this is a rules
//! action, and no player decides anything; the moves still go through the zone chokepoints,
//! so identities, knowledge and zone membership stay in agreement.

use crate::card::FaceIx;
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{CounterKind, MulliganStatus, ObjectFlags, Pending, State, TurnStep, Zone};
use crate::zone;

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
