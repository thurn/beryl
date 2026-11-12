//! Counters ([CR 122](https://yawgatog.com/resources/magic-rules/#R122)).

use beryl_card::{Amount, Objects};

use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layers;
use crate::layout::Layout;
use crate::state::{CounterKind, ObjectRef};

/// Puts `amount` counters of a kind on a permanent
/// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)). One that has changed zones
/// since gets none (CR 400.7).
#[inline]
pub fn add<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    if !game.state.is_current(object) || amount == 0 {
        return Ok(());
    }
    let layered = layers::counters_matter(game.state, game.table, object.slot, kind);
    let e = game.state.add_counters(object, kind, amount, layered)?;
    game.settle(e)
}

/// Removes up to `amount` counters of a kind from a permanent: as many as it has
/// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)).
#[inline]
pub fn remove<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    let amount = amount.min(game.state.counters_of(object.slot, kind));
    if amount == 0 {
        return Ok(());
    }
    let layered = layers::counters_matter(game.state, game.table, object.slot, kind);
    let e = game.state.remove_counters(object, kind, amount, layered);
    game.settle(e)
}

/// [`Effect::AddCounters`](beryl_card::Effect::AddCounters): "Put [count] [kind] counters on
/// [objects]" (CR 122.1). The outcome is the counters put.
pub(crate) fn add_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    kind: CounterKind,
    count: Amount,
) -> Result<Outcome<L>, Full> {
    change(run, objects, kind, count, add)
}

/// [`Effect::RemoveCounters`](beryl_card::Effect::RemoveCounters): "Remove [count] [kind]
/// counters from [objects]", as many as each has (CR 609.3). The outcome is the counters
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
    change: impl Fn(&mut Game<'_, L>, ObjectRef<L>, CounterKind, u8) -> Result<(), Full>,
) -> Result<Outcome<L>, Full> {
    let count = run.scene().amount(count);
    let permanents = run.permanents(objects);
    let counters = |run: &Run<'_, '_, L>| -> u32 {
        let s = run.state();
        permanents.sum(|slot| u32::from(s.counters_of(slot, kind)))
    };
    let before = counters(run);
    let mut left = permanents;
    while let Some(object) = run.next_permanent(&mut left) {
        change(run.game(), object, kind, count)?;
    }
    Ok(Outcome::of(permanents, counters(run).abs_diff(before)))
}
