//! Extra turns in the Effects store ([CR 500.7]): "Some effects can give a player extra
//! turns. They do this by adding the turns directly after the specified turn." An entry
//! ([`super::Payload::ExtraTurn`]) is on the player who takes the turn. Every effect in the
//! card pool adds its turn after the current one, so the entries waiting are the turns
//! directly after it, and the one created last comes first ([CR 500.7]: "The most recently
//! created turn will be taken first."). [`State::begin_turn`] takes that entry and begins
//! its player's turn; with none, the turn order goes on.
//!
//! [CR 500.7]: https://yawgatog.com/resources/magic-rules/#R5007

use std::cell::Cell;

use super::{Affects, Effects, Kinds, Payload};
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
#[cfg(doc)]
use crate::state::State;

impl<L: Layout> Effects<L> {
    /// Ends the extra turn created last and returns the player who takes it, the turn that
    /// begins now ([CR 500.7]): "The most recently created turn will be taken first." With
    /// none live, one AND.
    ///
    /// [CR 500.7]: https://yawgatog.com/resources/magic-rules/#R5007
    #[inline]
    pub(crate) fn take_extra_turn(&mut self) -> Option<Seat> {
        if self.kinds.0 & Kinds::EXTRA_TURN == 0 {
            return None;
        }
        Self::cold_take_extra_turn(self)
    }

    #[cold]
    #[inline(never)]
    fn cold_take_extra_turn(&mut self) -> Option<Seat> {
        // The entries are in the order they began, so the last is the one created last.
        let (index, player) = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match (e.affected, e.payload) {
                (Affects::Player(player), Payload::ExtraTurn) => Some((i, player)),
                _ => None,
            })
            .last()?;
        let at = Cell::new(0);
        self.end(|_| {
            let i = at.get();
            at.set(i + 1);
            i != index
        });
        Some(player)
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::{CardId, CardIx, Seat};
    use crate::layout::Small;
    use crate::profile::Profile;
    use crate::state::{Continuous, Duration, State};

    /// Gives `player` an extra turn after the current one.
    fn extra_turn(s: &mut State<Small>, player: Seat) {
        s.start_effect(
            CardIx::FIRST,
            Continuous::ExtraTurn(player),
            Duration::Indefinite,
        )
        .unwrap();
    }

    /// Begins turns until `n` have begun, and returns each one's active player.
    fn turns(s: &mut State<Small>, n: usize) -> Vec<Seat> {
        (0..n).map(|_| s.begin_turn()).collect()
    }

    /// CR 500.7: an extra turn comes directly after the current turn, and the turn order
    /// then goes on from the turn it was added after.
    #[test]
    fn an_extra_turn_comes_directly_after_the_current_one() {
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.set_starting_player(Seat::P0);
        s.begin_turn();
        extra_turn(&mut s, Seat::P0);
        assert_eq!(turns(&mut s, 3), [Seat::P0, Seat::P1, Seat::P0]);
        assert!(s.effects().is_empty(), "the turn took the effect");
        assert_eq!(s.turn().number(), 4);

        // An extra turn for the opponent: they take two turns in a row, then the order goes
        // on from theirs.
        extra_turn(&mut s, Seat::P1);
        assert_eq!(turns(&mut s, 3), [Seat::P1, Seat::P1, Seat::P0]);
    }

    /// CR 500.7: "The most recently created turn will be taken first." An extra turn
    /// created during an extra turn comes before those still waiting, and after them all
    /// the turn order goes on from the last turn that was not extra.
    #[test]
    fn the_most_recently_created_turn_is_taken_first() {
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.set_starting_player(Seat::P0);
        s.begin_turn();
        extra_turn(&mut s, Seat::P1);
        extra_turn(&mut s, Seat::P0);
        assert_eq!(s.begin_turn(), Seat::P0);
        extra_turn(&mut s, Seat::P0);
        assert_eq!(turns(&mut s, 4), [Seat::P0, Seat::P1, Seat::P1, Seat::P0]);
    }
}
