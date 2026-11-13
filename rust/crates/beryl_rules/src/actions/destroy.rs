//! Destroy ([CR 701.8](https://yawgatog.com/resources/magic-rules/#R7018)).

use beryl_card::{Objects, Regeneration};

use crate::actions::regenerate;
use crate::bitboard::Bitboard;
use crate::event::{self, Entry, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{ObjectRef, Zone};

/// Destroys a permanent ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)):
/// "To destroy a permanent, move it from the battlefield to its owner's graveyard." One that
/// has left the battlefield since is not destroyed (CR 400.7). A regeneration shield on it
/// replaces the destruction ([`regenerate::regenerate_any`]).
#[inline]
pub fn destroy<L: Layout>(game: &mut Game<'_, L>, object: ObjectRef<L>) -> Result<(), Full> {
    if !game.on_battlefield(object) {
        return Ok(());
    }
    let one = Bitboard::single(object.slot);
    if !regenerate::regenerate_any(game.state, game.table, game.sink, one)?.is_empty() {
        return Ok(());
    }
    let (to, reason) = (Zone::Graveyard, Reason::Destroy);
    event::move_one(game.state, game.table, game.sink, object.slot, to, reason)
}

/// Destroys a set of permanents simultaneously (CR 701.8a), as "Destroy each creature"
/// does: one event, so "whenever one or more" abilities see them together. Unless
/// `regeneration` forbids it, a regeneration shield replaces the destruction of the
/// permanent it is on ([CR 701.19c]): effects that say a permanent can't be regenerated
/// "cause regeneration shields to not be applied."
///
/// [CR 701.19c]: https://yawgatog.com/resources/magic-rules/#R70119c
#[inline]
pub fn destroy_all<L: Layout>(
    game: &mut Game<'_, L>,
    permanents: Bitboard<L::Slot>,
    regeneration: Regeneration,
) -> Result<(), Full> {
    let permanents = match regeneration {
        Regeneration::Allowed => {
            let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
            permanents & !regenerate::regenerate_any(s, t, sink, permanents)?
        }
        Regeneration::Forbidden => permanents,
    };
    let (to, reason) = (Zone::Graveyard, Reason::Destroy);
    let entry = Entry::default();
    event::move_batch(
        game.state, game.table, game.sink, permanents, to, reason, entry,
    )
}

/// [`Effect::Destroy`](beryl_card::Effect::Destroy): "Destroy [objects]." A regeneration
/// shield saves a permanent unless `regeneration` is
/// [`Regeneration::Forbidden`] ([`destroy_all`]). The outcome is the permanents that left the
/// battlefield, which a regenerated one has not.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    regeneration: Regeneration,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    if permanents.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(permanents)?;
    destroy_all(run.game(), permanents, regeneration)?;
    Ok(run.left_the_battlefield(permanents))
}
