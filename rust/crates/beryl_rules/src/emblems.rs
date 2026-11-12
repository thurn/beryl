//! Emblems ([CR 114](https://yawgatog.com/resources/magic-rules/#R114)).

use beryl_card::{CardDef, Players};

use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;

/// Gives a player the emblem that is the game-table card `card`
/// ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "'[Player] gets an
/// emblem with [ability].' This means that [player] puts an emblem with [ability] into the
/// command zone. The emblem is both owned and controlled by that player."
#[inline]
pub fn create<L: Layout>(game: &mut Game<'_, L>, player: Seat, card: L::Card) -> Result<(), Full> {
    game.state.add_emblem(player, card)
}

/// [`Effect::GetEmblem`](beryl_card::Effect::GetEmblem): "[Player] gets an emblem with
/// [abilities]" (CR 114.2). An emblem is no object the instructions after can name, so the
/// outcome is nothing.
///
/// # Panics
///
/// If the game's table lacks the emblem: a card lists its emblems among what it creates.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    emblem: &'static CardDef,
) -> Result<Outcome<L>, Full> {
    let card = run.game().table.index_of(emblem);
    let card = card.expect("an emblem missing from its card's tokens");
    for player in run.in_turn_order(players) {
        create(run.game(), player, card)?;
    }
    Ok(Outcome::NOTHING)
}
