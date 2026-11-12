//! Ending the game ([CR 104](https://yawgatog.com/resources/magic-rules/#R104)) as an
//! effect says.

use crate::event::LossCause;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::sba;

/// `player` wins the game by what is resolving
/// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
/// that a player wins the game." The game ends at once, and in a game that has ended
/// nothing more does.
pub fn win<L: Layout>(game: &mut Game<'_, L>, resolution: &Resolution<'_, L>, player: Seat) {
    if game.state.outcome().is_none() {
        let entry = &resolution.entry;
        let cause = LossCause::Effect {
            source: entry.object,
            card: entry.card,
            face: entry.face,
            ability: entry.ability,
        };
        sba::win_game(game.state, game.sink, player, cause);
    }
}
