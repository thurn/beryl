//! Shuffle ([CR 701.24](https://yawgatog.com/resources/magic-rules/#R70124)).

use beryl_card::{Objects, Players};

use crate::event::{Event, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::Zone;
use crate::zones;

/// Shuffles a player's library
/// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To shuffle a
/// library or a face-down pile of cards, randomize the cards within it so that no player
/// knows their order." The order comes from the game's stream for shuffles
/// ([`crate::chance`]).
pub fn shuffle<L: Layout>(game: &mut Game<'_, L>, player: Seat) {
    game.state.shuffle_library(player);
    game.sink.note(Event::LibraryShuffled { player });
}

/// [`Effect::ShuffleInto`](beryl_card::Effect::ShuffleInto): "[Player] shuffles [cards] into
/// their library." The cards go to their owners' libraries together
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)), and then each of the
/// players shuffles, in turn order
/// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)), whether or not any card
/// was theirs
/// ([CR 701.24d](https://yawgatog.com/resources/magic-rules/#R70124d)): "If an effect would
/// cause a player to shuffle a set of objects into a library, that library is shuffled even
/// if there are no objects in that set." The outcome is the cards that are in a library.
pub(crate) fn into_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    cards: Objects,
) -> Result<Outcome<L>, Full> {
    let cards = run.elsewhere_than(cards, Zone::Library);
    zones::move_all(run.game(), cards, Zone::Library, Reason::Effect)?;
    for player in run.in_turn_order(players) {
        shuffle(run.game(), player);
    }
    Ok(run.now_in(cards, Zone::Library))
}
