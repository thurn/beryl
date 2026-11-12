//! Flipping a coin ([CR 705](https://yawgatog.com/resources/magic-rules/#R705)).

use beryl_card::Players;

use crate::bitboard::Bitboard;
use crate::chance;
use crate::event::Event;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::ObjectRef;

/// `player` flips a coin for an effect of `source`. Returns whether they won the flip
/// ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "the player that flips
/// the coin calls 'heads' or 'tails.' If the call matches the result, the player wins the
/// flip." A fair coin against any call is a fair win, so one draw from the game's
/// randomness decides it (architecture §14), or while a save replays, its record
/// ([`chance`]).
pub fn flip<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    source: ObjectRef<L>,
) -> Result<bool, Full> {
    let won = chance::flip(game.state, game.sink, player);
    game.settle(Event::CoinFlip {
        player,
        won,
        source,
    })?;
    Ok(won)
}

/// [`Effect::FlipCoin`](beryl_card::Effect::FlipCoin): "[Player] flips a coin" (CR 705.2),
/// for the source of what is resolving. The outcome is the flips won, so it did something
/// when the player won theirs
/// ([`Condition::WonFlip`](beryl_card::Condition::WonFlip)).
///
/// # Panics
///
/// For an emblem's ability, which has no source.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
) -> Result<Outcome<L>, Full> {
    let source = run.resolution().source();
    let mut won = 0;
    for player in run.in_turn_order(players) {
        won += u32::from(flip(run.game(), player, source)?);
    }
    Ok(Outcome::of(Bitboard::EMPTY, won))
}
