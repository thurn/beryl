//! Flipping a coin ([CR 705](https://yawgatog.com/resources/magic-rules/#R705)).

use crate::chance;
use crate::event::Event;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
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
