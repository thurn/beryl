//! Tap and untap ([CR 701.26](https://yawgatog.com/resources/magic-rules/#R70126)).

use crate::card::{Flow, Prompt, Purpose};
use crate::event;
use crate::filter::{Evaluate, Filter};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::resolution::Resolution;
use crate::state::{ObjectRef, Zone};

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

/// `player` untaps up to `n` permanents of their choice that pass `filter`, whoever controls
/// them: "untap up to two lands". The filter's "you" is the controller of what is resolving.
/// Untapping an untapped permanent does nothing (CR 701.26b: "Only tapped permanents can be
/// untapped"), so only tapped ones are offered.
pub fn untap_up_to<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    filter: Filter,
    n: u8,
) -> Flow {
    let (s, you) = (&*game.state, resolution.controller());
    let tapped = filter
        .select(s, game.table, s.in_zone(Zone::Battlefield), you)
        .filter(|slot| s.object(slot).is_tapped());
    let prompt = Prompt::new(Purpose::Untap).most(n);
    let chosen = resolution.choose_up_to(game, player, tapped, n, prompt)?;
    // In battlefield order, never slot order: the untaps' events are seen in this order.
    let permanents = game.state.in_game_order(game.table, chosen)?;
    for &slot in permanents.as_slice() {
        untap(game, game.state.object_ref(slot))?;
    }
    Ok(())
}
