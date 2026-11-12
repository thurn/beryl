//! Sacrifice ([CR 701.21](https://yawgatog.com/resources/magic-rules/#R70121)).

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{ObjectRef, Zone};
use crate::zones;

/// Sacrifices a permanent ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)):
/// "To sacrifice a permanent, its controller moves it from the battlefield directly to its
/// owner's graveyard." One that has left the battlefield since is not sacrificed (CR 400.7).
#[inline]
pub fn sacrifice<L: Layout>(game: &mut Game<'_, L>, object: ObjectRef<L>) -> Result<(), Full> {
    if !game.on_battlefield(object) {
        return Ok(());
    }
    zones::move_to(game, object, Zone::Graveyard, Reason::Sacrifice)
}

/// Sacrifices a set of permanents simultaneously (CR 701.21a): one event, so "whenever one
/// or more" abilities see them together.
#[inline]
pub fn sacrifice_all<L: Layout>(
    game: &mut Game<'_, L>,
    permanents: Bitboard<L::Slot>,
) -> Result<(), Full> {
    zones::move_all(game, permanents, Zone::Graveyard, Reason::Sacrifice)
}

/// [`Effect::Sacrifice`](beryl_card::Effect::Sacrifice): "Sacrifice [objects]" (CR 701.21a).
/// The outcome is the permanents that left the battlefield.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    if permanents.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(permanents)?;
    sacrifice_all(run.game(), permanents)?;
    Ok(run.left_the_battlefield(permanents))
}
