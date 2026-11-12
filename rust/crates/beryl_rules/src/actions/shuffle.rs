//! Shuffle ([CR 701.24](https://yawgatog.com/resources/magic-rules/#R70124)).

use crate::event::Event;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;

/// Shuffles a player's library
/// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To shuffle a
/// library or a face-down pile of cards, randomize the cards within it so that no player
/// knows their order." The order comes from the game's stream for shuffles
/// ([`crate::chance`]).
pub fn shuffle<L: Layout>(game: &mut Game<'_, L>, player: Seat) {
    game.state.shuffle_library(player);
    game.sink.note(Event::LibraryShuffled { player });
}
