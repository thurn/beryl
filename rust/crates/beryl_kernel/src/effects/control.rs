//! Player-controlling effects in the Effects store (see [CR 724.1]): "Some cards allow a player to
//! control another player during that player's next turn." An entry
//! ([`super::Payload::Control`], the controller's seat) is on the controlled player and waits
//! for that player's next turn: [`State::begin_turn`] takes the one created last
//! ([CR 724.1a]), ends every one on that player, and makes its controller the turn's
//! ([`crate::state::Turn::controller`]), which the next turn's beginning clears. The
//! controller then answers that player's decisions ([`State::decider`]) and learns what they
//! learn, from the turn's start on, and keeps it (see [CR 724.4]; `State::sharer`).
//!
//! [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
//! [CR 724.1a]: https://yawgatog.com/resources/magic-rules/#R7241a
//! [CR 724.4]: https://yawgatog.com/resources/magic-rules/#R7244
//!
//! Implements: CR 724.1a, CR 724.9

use super::{Affects, ContinuousEffect, Effects, Kinds, Payload};
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
#[cfg(doc)]
use crate::state::State;

impl<L: Layout> Effects<L> {
    /// Ends every player-controlling effect waiting for `player`'s turn, which begins now,
    /// and returns the controller of the one created last, unless that is `player`
    /// ([CR 724.1a]): "Multiple player-controlling effects that affect the same player
    /// overwrite each other. The last one to be created is the one that works." A player who
    /// controls themselves still makes their own decisions ([CR 724.9]): "That player will make
    /// their own decisions and choices as normal." With no effects waiting, one AND.
    ///
    /// [CR 724.1a]: https://yawgatog.com/resources/magic-rules/#R7241a
    /// [CR 724.9]: https://yawgatog.com/resources/magic-rules/#R7249
    #[inline]
    pub(crate) fn take_control(&mut self, player: Seat) -> Option<Seat> {
        if self.kinds.0 & Kinds::CONTROL == 0 {
            return None;
        }
        Self::cold_take_control(self, player)
    }

    #[cold]
    #[inline(never)]
    fn cold_take_control(&mut self, player: Seat) -> Option<Seat> {
        let on = |e: &ContinuousEffect<L>| match (e.affected, e.payload) {
            (Affects::Player(p), Payload::Control(controller)) if p == player => Some(controller),
            _ => None,
        };
        // The entries are in the order they began, so the last is the one created last.
        let controller = self.entries.iter().filter_map(on).last()?;
        self.end(|e| on(e).is_none());
        (controller != player).then_some(controller)
    }
}

#[cfg(test)]
mod tests {
    use crate::bitboard::Bitboard;
    use crate::event::Sink;
    use crate::ids::{CardId, CardIx, Seat, Slot, SlotId};
    use crate::layout::Small;
    use crate::profile::Profile;
    use crate::state::{Continuous, Duration, State};

    /// Starts a player-controlling effect: `by` controls `whom`.
    fn control(s: &mut State<Small>, whom: Seat, by: Seat) {
        let control = Continuous::Control {
            controlled: whom,
            controller: by,
        };
        s.start_effect(CardIx::FIRST, control, Duration::Indefinite)
            .unwrap();
    }

    /// CR 724.1: the effect waits for the controlled player's turn, gives the controller
    /// that turn's decisions, and ends as the turn after begins. The opponent's turn in
    /// between is not controlled.
    #[test]
    fn the_controlled_turn_is_the_next_one_the_player_takes() {
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.set_starting_player(Seat::P0);
        assert_eq!(s.begin_turn().unwrap(), Seat::P0);
        control(&mut s, Seat::P1, Seat::P0);
        assert_eq!(s.decider(Seat::P1), Seat::P1, "it waits for P1's turn");

        assert_eq!(s.begin_turn().unwrap(), Seat::P1);
        assert_eq!(s.turn().controller(), Some(Seat::P0));
        assert_eq!(s.decider(Seat::P1), Seat::P0);
        // CR 724.8: the controller still makes their own decisions.
        assert_eq!(s.decider(Seat::P0), Seat::P0);
        assert!(s.effects().is_empty(), "the turn took the effect");

        s.begin_turn().unwrap();
        s.begin_turn().unwrap();
        assert_eq!(s.turn().active(), Seat::P1);
        assert_eq!(
            (s.turn().controller(), s.decider(Seat::P1)),
            (None, Seat::P1)
        );
    }

    /// CR 724.1a: the last effect created works; CR 724.9: a player who controls themself
    /// decides for themself.
    #[test]
    fn the_last_effect_created_works() {
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.set_starting_player(Seat::P0);
        s.begin_turn().unwrap();
        control(&mut s, Seat::P1, Seat::P0);
        control(&mut s, Seat::P1, Seat::P1);
        s.begin_turn().unwrap();
        assert_eq!(s.turn().controller(), None);
        assert!(s.effects().is_empty());

        control(&mut s, Seat::P0, Seat::P0);
        control(&mut s, Seat::P0, Seat::P1);
        s.begin_turn().unwrap();
        assert_eq!(s.decider(Seat::P0), Seat::P1);
    }

    /// CR 724.4: the controller knows what the controlled player knows during the turn, and
    /// keeps knowing it after.
    #[test]
    fn the_controller_sees_what_the_controlled_player_sees() {
        let [seen, later] = [Slot::from_index(3), Slot::from_index(4)];
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.set_starting_player(Seat::P0);
        s.begin_turn().unwrap();
        s.board().set_known(Seat::P1, seen, true);
        control(&mut s, Seat::P1, Seat::P0);
        assert!(!s.knows(Seat::P0, seen));

        s.begin_turn().unwrap();
        assert!(s.knows(Seat::P0, seen));
        s.look(&mut Sink::default(), Seat::P1, Bitboard::single(later));
        assert!(s.knows(Seat::P0, later));
        assert!(!s.knows(Seat::P1, Slot::from_index(5)));

        s.begin_turn().unwrap();
        assert!(s.knows(Seat::P0, seen) && s.knows(Seat::P0, later));
        assert_eq!(
            s.knowledge()[Seat::P0.index()] & Bitboard::single(seen),
            Bitboard::single(seen)
        );
    }
}
