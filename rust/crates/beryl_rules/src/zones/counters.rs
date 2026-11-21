//! Counters (see [CR 122](https://yawgatog.com/resources/magic-rules/#R122)).
//!
//! Implements: CR 122.6

use beryl_card::{Amount, Objects};
use beryl_kernel::reference::Permanent;

use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::{CounterKind, ObjectRef};

/// Puts `amount` counters of a kind on a permanent
/// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)). One that has changed zones
/// since gets none (see CR 400.7).
#[inline]
pub fn add<L: Layout>(
    game: &mut Game<'_, L>,
    object: Permanent<L>,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    let Some(object) = game.state.current(ObjectRef::from(object)) else {
        return Ok(());
    };
    let slot = object.reference().slot;
    if amount == 0 || !game.state.object(slot).on_battlefield() {
        return Ok(());
    }
    let layered = game.state.layers_read_counters(game.table, slot, kind);
    let e = game.state.add_counters(object, kind, amount, layered)?;
    game.settle(e)
}

/// Removes up to `amount` counters of a kind from a permanent: as many as it has
/// (see [CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)).
#[inline]
pub fn remove<L: Layout>(
    game: &mut Game<'_, L>,
    object: Permanent<L>,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    let object = ObjectRef::from(object);
    if !game.state.object(object.slot).on_battlefield() {
        return Ok(());
    }
    remove_from_object(game, object, kind, amount)
}

/// Removes counters from an object whose own ability can use them outside the battlefield
/// (see [CR 113.6]). The caller's cost determines the source's zone.
///
/// [CR 113.6]: https://yawgatog.com/resources/magic-rules/#R1136
#[inline]
pub(crate) fn remove_from_object<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    let Some(object) = game.state.current(object) else {
        return Ok(());
    };
    let slot = object.reference().slot;
    let amount = amount.min(game.state.counters_of(slot, kind));
    if amount == 0 {
        return Ok(());
    }
    let layered = game.state.layers_read_counters(game.table, slot, kind);
    let e = game.state.remove_counters(object, kind, amount, layered);
    game.settle(e)
}

/// [`Effect::AddCounters`](beryl_card::Effect::AddCounters): "Put [count] [kind] counters on
/// [objects]" (see CR 122.1). The outcome is the counters put.
pub(crate) fn add_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    kind: CounterKind,
    count: Amount,
) -> Result<Outcome<L>, Full> {
    change(run, objects, kind, count, add)
}

/// [`Effect::RemoveCounters`](beryl_card::Effect::RemoveCounters): "Remove [count] [kind]
/// counters from [objects]", as many as each has (see CR 609.3). The outcome is the counters
/// removed.
pub(crate) fn remove_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    kind: CounterKind,
    count: Amount,
) -> Result<Outcome<L>, Full> {
    change(run, objects, kind, count, remove)
}

/// Puts counters on each permanent `objects` names, or removes them, with `change`, in the
/// order the permanents entered the battlefield.
fn change<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    kind: CounterKind,
    count: Amount,
    change: impl Fn(&mut Game<'_, L>, Permanent<L>, CounterKind, u8) -> Result<(), Full>,
) -> Result<Outcome<L>, Full> {
    let count = run.scene().amount(count);
    let permanents = run.permanents(objects);
    let counters = |run: &Run<'_, '_, L>| -> u32 {
        let s = run.state();
        permanents
            .slots()
            .sum(|slot| u32::from(s.counters_of(slot, kind)))
    };
    let before = counters(run);
    let mut left = permanents;
    while let Some(object) = run.next_permanent(&mut left) {
        change(run.game(), object, kind, count)?;
    }
    Ok(Outcome::of(
        permanents.slots(),
        counters(run).abs_diff(before),
    ))
}
