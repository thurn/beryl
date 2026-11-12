//! Tap and untap ([CR 701.26](https://yawgatog.com/resources/magic-rules/#R70126)).

use beryl_card::{Amount, Objects, Players};

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::choices;
use crate::event;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::resolution::Resolution;
use crate::state::ObjectRef;

/// Taps a permanent ([CR 701.26a](https://yawgatog.com/resources/magic-rules/#R70126a)): "To
/// tap a permanent, turn it sideways from an upright position. Only untapped permanents can
/// be tapped."
#[inline]
pub fn tap<L: Layout>(game: &mut Game<'_, L>, object: ObjectRef<L>) -> Result<(), Full> {
    event::set_tapped(game.state, game.table, game.sink, object, true)
}

/// Untaps a permanent ([CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b)):
/// "To untap a permanent, rotate it back to the upright position from a sideways position.
/// Only tapped permanents can be untapped."
#[inline]
pub fn untap<L: Layout>(game: &mut Game<'_, L>, object: ObjectRef<L>) -> Result<(), Full> {
    event::set_tapped(game.state, game.table, game.sink, object, false)
}

/// `player` untaps up to `n` of `permanents`, of their choice, whoever controls them: "untap
/// up to two lands". Untapping an untapped permanent does nothing (CR 701.26b: "Only tapped
/// permanents can be untapped"), so only tapped ones are offered. Returns those untapped.
pub fn untap_up_to<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    permanents: Bitboard<L::Slot>,
    n: u8,
) -> Result<Bitboard<L::Slot>, Stop> {
    let s = &*game.state;
    let tapped = permanents.filter(|slot| s.object(slot).is_tapped());
    let prompt = Prompt::new(Purpose::Untap).most(n);
    let chosen = choices::pick_up_to(game, resolution, player, tapped, n, prompt)?;
    // In battlefield order, never slot order: the untaps' events are seen in this order.
    let permanents = game.state.in_game_order(game.table, chosen)?;
    for &slot in permanents.as_slice() {
        untap(game, game.state.object_ref(slot))?;
    }
    Ok(chosen)
}

/// [`Effect::UntapUpTo`](beryl_card::Effect::UntapUpTo): "[Player] untaps up to [count]
/// [permanents]" (CR 701.26b), chosen as the instruction is reached
/// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)). The outcome is the
/// permanents untapped.
pub(crate) fn untap_up_to_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    count: Amount,
    permanents: Objects,
) -> Result<Outcome<L>, Stop> {
    let count = run.scene().amount(count);
    let mut untapped = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let permanents = run.permanents(permanents);
        let (game, resolution) = run.resolving();
        untapped |= untap_up_to(game, resolution, player, permanents, count)?;
    }
    Ok(Outcome::acted_on(untapped))
}
