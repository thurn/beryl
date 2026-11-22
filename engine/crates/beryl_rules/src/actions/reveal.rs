//! Reveal (see [CR 701.20](https://yawgatog.com/resources/magic-rules/#R70120)).
//!
//! Implements: none. Delegates revealing a card to the kernel's knowledge store.

use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::state::Zone;
use beryl_card::Objects;

/// Reveals a card to every player
/// (see [CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
/// show that card to all players for a brief time." Each player knows it from then on,
/// until it is shuffled or otherwise hidden anew.
#[inline]
pub fn reveal<L: Layout>(game: &mut Game<'_, L>, player: Seat, card: L::Slot) {
    game.state.reveal(game.sink, player, card);
}

/// Reveals the named cards without moving them. The next instruction can refer to the cards
/// shown this way (see CR 701.20a).
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    cards: Objects,
) -> Result<Outcome<L>, Full> {
    let cards = run.elsewhere_than(cards, Zone::Nowhere);
    let player = run.resolution().controller();
    for &slot in run.in_game_order(cards)?.iter() {
        reveal(run.game(), player, slot);
    }
    Ok(Outcome::acted_on(cards))
}
