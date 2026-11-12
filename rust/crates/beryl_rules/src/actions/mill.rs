//! Mill ([CR 701.17](https://yawgatog.com/resources/magic-rules/#R70117)).

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{Zone, ZoneKey};
use crate::{library, zones};

/// `player` mills `n` cards
/// ([CR 701.17a](https://yawgatog.com/resources/magic-rules/#R70117a)): "For a player to
/// mill a number of cards, that player puts that many cards from the top of their library
/// into their graveyard." [CR 701.17b](https://yawgatog.com/resources/magic-rules/#R70117b):
/// "A player can't mill a number of cards greater than the number of cards in their
/// library." Returns the cards that were on top.
pub fn mill<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    n: u8,
) -> Result<Bitboard<L::Slot>, Full> {
    let top = library::top(game.state, player, n);
    zones::move_all(game, top, Zone::Graveyard, Reason::Effect)?;
    Ok(top)
}

/// [`Effect::Mill`](beryl_card::Effect::Mill): "[Player] mills [count] cards" (CR 701.17a).
/// The outcome is the cards that left the library.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    count: Amount,
) -> Result<Outcome<L>, Full> {
    let count = run.scene().amount(count);
    let mut milled = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let game = run.game();
        let top = mill(game, player, count)?;
        milled |= top & !game.state.zone(ZoneKey::Of(player, Zone::Library));
    }
    Ok(Outcome::acted_on(milled))
}
