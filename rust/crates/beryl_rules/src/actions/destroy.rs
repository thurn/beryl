//! Destroy ([CR 701.8](https://yawgatog.com/resources/magic-rules/#R7018)).

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::event::{self, Entry, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{ObjectRef, Zone};

/// Destroys a permanent ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)):
/// "To destroy a permanent, move it from the battlefield to its owner's graveyard." One that
/// has left the battlefield since is not destroyed (CR 400.7).
#[inline]
pub fn destroy<L: Layout>(game: &mut Game<'_, L>, object: ObjectRef<L>) -> Result<(), Full> {
    if !game.on_battlefield(object) {
        return Ok(());
    }
    let (to, reason) = (Zone::Graveyard, Reason::Destroy);
    event::move_one(game.state, game.table, game.sink, object.slot, to, reason)
}

/// Destroys a set of permanents simultaneously (CR 701.8a), as "Destroy each creature"
/// does: one event, so "whenever one or more" abilities see them together.
#[inline]
pub fn destroy_all<L: Layout>(
    game: &mut Game<'_, L>,
    permanents: Bitboard<L::Slot>,
) -> Result<(), Full> {
    let (to, reason) = (Zone::Graveyard, Reason::Destroy);
    let entry = Entry::default();
    event::move_batch(
        game.state, game.table, game.sink, permanents, to, reason, entry,
    )
}

/// [`Effect::Destroy`](beryl_card::Effect::Destroy): "Destroy [objects]." The outcome is the
/// permanents that left the battlefield.
///
/// Its `regeneration` is not read: the engine has no regeneration shields yet, so every
/// destruction already behaves as "can't be regenerated" does
/// ([CR 701.19c](https://yawgatog.com/resources/magic-rules/#R70119c)): "they cause
/// regeneration shields to not be applied." A regeneration shield, when one is added, must not
/// apply to a destruction with [`Regeneration::Forbidden`](beryl_card::Regeneration::Forbidden).
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    if permanents.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(permanents)?;
    destroy_all(run.game(), permanents)?;
    Ok(run.left_the_battlefield(permanents))
}
