//! Fateseal ([CR 701.29](https://yawgatog.com/resources/magic-rules/#R70129)).

use beryl_card::{Amount, Players, prompt};

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::choices;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::zone::Place;
use crate::{library, zones};

/// `player` looks at the top card of `whose` library and may put it on the bottom of that
/// library: to fateseal 1, of any player's library
/// ([CR 701.29a](https://yawgatog.com/resources/magic-rules/#R70129a)): "To 'fateseal N'
/// means to look at the top N cards of an opponent's library, then put any number of them on
/// the bottom of that library in any order and the rest on top of that library in any
/// order." Looking shows the card to `player` alone, an exception to
/// [CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Players can't look at or
/// change the order of cards in a library." Moving it within the library is no zone change,
/// so it stays the same object. A yes puts it on the bottom; with no card there, nothing is
/// asked. Returns the card looked at.
pub fn fateseal<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    whose: Seat,
) -> Result<Bitboard<L::Slot>, Stop> {
    let looked = library::top(game.state, whose, 1);
    let Some(top) = looked.sole() else {
        return Ok(looked);
    };
    let top = game.state.object_ref(top);
    zones::look_at(game, player, looked);
    let fateseal = Prompt::new(Purpose::Fateseal)
        .count(1)
        .places(prompt::Place::Bottom, prompt::Place::Top);
    if choices::yes_no(game, resolution, player, fateseal, looked)? {
        library::put_into(game, top, Place::Bottom)?;
    }
    Ok(looked)
}

/// [`Effect::Fateseal`](beryl_card::Effect::Fateseal): "[Player] looks at the top card of
/// [library]'s library and may put it on the bottom of that library" (CR 701.29a). One card:
/// the definition check refuses any other count. The outcome is the cards looked at.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    library: Players,
    count: Amount,
) -> Result<Outcome<L>, Stop> {
    debug_assert_eq!(
        run.scene().amount(count),
        1,
        "the engine fateseals one card"
    );
    let mut looked = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        for whose in run.in_turn_order(library) {
            let (game, resolution) = run.resolving();
            looked |= fateseal(game, resolution, player, whose)?;
        }
    }
    Ok(Outcome::acted_on(looked))
}
