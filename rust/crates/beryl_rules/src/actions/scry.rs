//! Scry ([CR 701.22](https://yawgatog.com/resources/magic-rules/#R70122)).

use beryl_card::{Amount, Players, prompt};

use crate::bitboard::Bitboard;
use crate::card::{Flow, Part, Prompt, Purpose, Stop};
use crate::choices;
use crate::event::Event;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::library;
use crate::resolution::Resolution;
use crate::zone::Place;

/// `player` scries `n`
/// ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "To 'scry N' means
/// to look at the top N cards of your library, then put any number of them on the bottom
/// of your library in any order and the rest on top of your library in any order."
///
/// The cards for the bottom are chosen one at a time, each going under the ones before,
/// until the player is done; then those left are chosen one at a time from the top down.
/// Every arrangement is one sequence of answers. [CR 701.22b](https://yawgatog.com/resources/magic-rules/#R70122b):
/// "If a player is instructed to scry 0, no scry event occurs."
pub fn scry<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    n: u8,
) -> Flow {
    // One prompt: the cards for the bottom, at most all of them, then the order of the
    // rest on top.
    let scry = Prompt::new(Purpose::Scry)
        .count(n)
        .places(prompt::Place::Bottom, prompt::Place::Top);
    let mut left = library::top(game.state, player, n);
    game.state.look(game.sink, player, left);
    let mut bottom = 0;
    while !left.is_empty() {
        let bottom_pick = scry
            .most(u8::try_from(left.count()).unwrap_or(n))
            .part(Part::First);
        let Some(slot) = choices::pick_or_end(game, resolution, player, left, bottom_pick)? else {
            break;
        };
        game.state.reorder_library(slot, Place::Bottom)?;
        left.remove(slot);
        bottom += 1;
    }
    let mut order: L::Queue<L::Slot, 60> = Default::default();
    while left.count() > 1 {
        let slot = choices::pick(game, resolution, player, left, scry.part(Part::Second))?;
        left.remove(slot);
        order.push(slot)?;
    }
    if let Some(slot) = left.sole() {
        order.push(slot)?;
    }
    for &slot in order.iter().rev() {
        game.state.reorder_library(slot, Place::Top)?;
    }
    if n > 0 {
        game.sink.note(Event::Scry {
            player,
            top: u8::try_from(order.len()).expect("scry top fits in u8"),
            bottom,
        });
    }
    Ok(())
}

/// [`Effect::Scry`](beryl_card::Effect::Scry): "[Player] scries [count]." The outcome is the
/// number of cards looked at.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    count: Amount,
) -> Result<Outcome<L>, Stop> {
    let count = run.scene().amount(count);
    let mut looked = 0;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        looked += library::top(game.state, player, count).count();
        scry(game, resolution, player, count)?;
    }
    Ok(Outcome::of(Bitboard::EMPTY, looked))
}
