//! One-shot effects (see [CR 610](https://yawgatog.com/resources/magic-rules/#R610)): those that
//! move an object "until" an event, which a second one then undoes (CR 610.3).
//!
//! Implements: CR 610.3

use beryl_card::Objects;
use beryl_kernel::reference::Permanent;

use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement::event::Reason;
use crate::state::{Memo, ObjectRef, Zone};
use crate::zones;

/// Exiles a permanent until `until`, another permanent, leaves the battlefield
/// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot effects
/// cause an object to change zones 'until' a specified event occurs. A second one-shot
/// effect is created immediately after the specified event. This second one-shot effect
/// returns the object to its previous zone." The move that takes `until` off the battlefield
/// makes that second effect ([`crate::replacement::event`]).
///
/// [CR 610.3b](https://yawgatog.com/resources/magic-rules/#R6103b): "If a resolving
/// triggered ability creates the initial one-shot effect that causes the object to change
/// zones, and the specified event has already occurred before that one-shot effect would
/// occur but after that ability triggered, the object doesn't move."
pub fn exile_until_leaves<L: Layout>(
    game: &mut Game<'_, L>,
    object: Permanent<L>,
    until: ObjectRef<L>,
) -> Result<(), Full> {
    // A phased-out `until` has not left the battlefield (see CR 702.26d), so the event has not
    // occurred; a phased-out `object` can't be exiled (see CR 702.26b).
    let until_stays = game.state.current(until).is_some()
        && game.state.object(until.slot).zone() == Zone::Battlefield;
    if !until_stays || game.state.permanent(object.into()).is_none() {
        return Ok(());
    }
    zones::move_to(game, object.into(), Zone::Exile, Reason::Exile)?;
    if game.state.object(object.slot()).zone() == Zone::Exile {
        (game.state).remember(object.slot(), Memo::ExiledUntil(until.slot))?;
    }
    Ok(())
}

/// [`Effect::ExileUntilThisLeaves`](beryl_card::Effect::ExileUntilThisLeaves): "Exile
/// [objects] until this leaves the battlefield" (CR 610.3), each in the order they entered
/// the battlefield. The outcome is the permanents that left it.
///
/// # Panics
///
/// For an emblem's ability, which has no source.
pub(crate) fn exile_until_this_leaves_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    run.remember(permanents.slots())?;
    let mut left = permanents;
    while let Some(object) = run.state().next_permanent(&mut left) {
        let source = run.resolution().source();
        exile_until_leaves(run.game(), object, source)?;
    }
    Ok(run.left_the_battlefield(permanents.slots()))
}
