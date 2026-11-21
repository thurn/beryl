//! Reveal (see [CR 701.20](https://yawgatog.com/resources/magic-rules/#R70120)).
//!
//! Implements: none. Delegates revealing a card to the kernel's knowledge store.

use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;

/// Reveals a card to every player
/// (see [CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
/// show that card to all players for a brief time." Each player knows it from then on,
/// until it is shuffled or otherwise hidden anew.
#[inline]
pub fn reveal<L: Layout>(game: &mut Game<'_, L>, player: Seat, card: L::Slot) {
    game.state.reveal(game.sink, player, card);
}
