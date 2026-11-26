//! Who runs out of cards first. A player who must draw from an empty library loses
//! ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)): "If a player attempted to
//! draw a card from a library with no cards in it since the last time state-based actions were
//! checked, that player loses the game." In the shared-deck format both players draw from one
//! library, so who loses is whoever must draw first once it is empty, which the draws' turn
//! order decides, not who has the shorter library.

use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Layout, Seat, State, TurnStep};

/// The player whose draw step comes next: the active player before their draw step, unless it
/// is the starting player's first turn, whose draw step is skipped
/// ([CR 103.8a](https://yawgatog.com/resources/magic-rules/#R1038a)): "the player who plays
/// first skips the draw step"; otherwise the other player. Before the first turn, the player who does not start draws first.
pub fn next_drawer<L: Layout>(s: &State<L>) -> Seat {
    let turn = s.turn();
    if turn.number() == 0 {
        return turn.starting_player().opponent();
    }
    let active = turn.active();
    if turn.step() < TurnStep::Draw && !turn.is_starting_players_first_turn() {
        active
    } else {
        active.opponent()
    }
}

/// The player who first draws from an empty library when each player draws one card in each of
/// their draw steps and nothing else takes a card. With libraries of their own, that is the
/// player with fewer cards, or the next to draw ([`next_drawer`]) when the two hold as many. With
/// a shared library the draws alternate from one pile, starting with the next to draw, and the
/// draw after its last card is the one that loses: the next to draw when the pile holds an even
/// number of cards, the other player when it holds an odd number.
pub fn first_to_deck<L: Layout>(s: &State<L>) -> Seat {
    let next = next_drawer(s);
    let theirs = next.opponent();
    if s.shares_zones() {
        if s.library(next).len().is_multiple_of(2) {
            next
        } else {
            theirs
        }
    } else if s.library(next).len() <= s.library(theirs).len() {
        next
    } else {
        theirs
    }
}
