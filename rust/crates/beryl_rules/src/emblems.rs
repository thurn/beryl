//! Emblems ([CR 114](https://yawgatog.com/resources/magic-rules/#R114)).

use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;

/// Gives a player the emblem that is the game-table card `card`
/// ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "'[Player] gets an
/// emblem with [ability].' This means that [player] puts an emblem with [ability] into the
/// command zone. The emblem is both owned and controlled by that player."
#[inline]
pub fn create<L: Layout>(game: &mut Game<'_, L>, player: Seat, card: L::Card) -> Result<(), Full> {
    game.state.add_emblem(player, card)
}
