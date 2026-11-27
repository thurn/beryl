//! Counters (see [CR 122](https://yawgatog.com/resources/magic-rules/#R122)).
//!
//! Implements: CR 122.6, CR 701.10e

use beryl_card::{Amount, Objects};
use beryl_kernel::reference::{Permanent, Permanents};

use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement;
use crate::state::{CounterKind, ObjectRef};

/// Puts `amount` counters of a kind on a permanent
/// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)), as many as the
/// replacement effects of counters leave ([`replacement::counters`]). `by_effect` says whether
/// an effect puts them, rather than a cost or a rule, which some of those replacements ask.
/// One that has changed zones since gets none (see CR 400.7).
#[inline]
pub fn add<L: Layout>(
    game: &mut Game<'_, L>,
    object: Permanent<L>,
    kind: CounterKind,
    amount: u8,
    by_effect: bool,
) -> Result<(), Full> {
    let Some(object) = game.state.current(ObjectRef::from(object)) else {
        return Ok(());
    };
    let slot = object.reference().slot;
    if amount == 0 || !game.state.object(slot).on_battlefield() {
        return Ok(());
    }
    let amount = replacement::counters(
        game.state,
        game.table,
        game.sink,
        replacement::Counters {
            on: slot,
            kind,
            amount,
            by_effect,
        },
    )?;
    if kind == CounterKind::Lore {
        return add_lore(game, object, amount);
    }
    let layered = game.state.layers_read_counters(game.table, slot, kind);
    let e = game.state.add_counters(object, kind, amount, layered)?;
    game.settle(e)
}

/// Lore placement also matches any chapter thresholds it crosses (see CR 715.2b).
#[cold]
#[inline(never)]
fn add_lore<L: Layout>(
    game: &mut Game<'_, L>,
    object: beryl_kernel::reference::Current<ObjectRef<L>>,
    amount: u8,
) -> Result<(), Full> {
    let slot = object.reference().slot;
    let kind = CounterKind::Lore;
    let before = game.state.counters_of(slot, kind);
    let layered = game.state.layers_read_counters(game.table, slot, kind);
    let e = game.state.add_counters(object, kind, amount, layered)?;
    game.settle(e)?;
    if game.table.can(crate::table::Capability::Saga) {
        crate::saga::match_lore(game.state, game.table, slot, before)?;
    }
    Ok(())
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
    let Some(object) = game.state.permanent(object.into()) else {
        return Ok(());
    };
    remove_from_object(game, object.into(), kind, amount)
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
    // An effect puts these counters.
    change(run, objects, kind, count, |game, object, kind, n| {
        add(game, object, kind, n, true)
    })
}

/// [`Effect::DoubleCounters`](beryl_card::Effect::DoubleCounters): "Double the number of
/// [kind] counters on [objects]" ([CR 701.10e]): "To double the number of a kind of counters on
/// a player or permanent, give that player or permanent as many of those counters as that
/// player or permanent already has." Each permanent is given its counters by an effect, so the
/// replacement effects of counters apply to each placement. The outcome is the counters put.
///
/// [`Full`] when the number given does not fit beside the number it has.
///
/// [CR 701.10e]: https://yawgatog.com/resources/magic-rules/#R70110e
pub(crate) fn double<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    kind: CounterKind,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    let before = counters_on(run, permanents, Some(kind));
    let mut left = permanents;
    while let Some(object) = run.state().next_permanent(&mut left) {
        let has = run.state().counters_of(object.slot(), kind);
        has.checked_add(has).ok_or(Full)?;
        add(run.game(), object, kind, has, true)?;
    }
    let after = counters_on(run, permanents, Some(kind));
    Ok(Outcome::of(permanents.slots(), after.abs_diff(before)))
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

/// [`Effect::RemoveAllCounters`](beryl_card::Effect::RemoveAllCounters): "Remove all
/// counters from [objects]", every kind each has (see CR 122.1). The outcome is the counters
/// removed.
pub(crate) fn remove_all_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    let before = counters_on(run, permanents, None);
    let mut left = permanents;
    while let Some(object) = run.state().next_permanent(&mut left) {
        // Each kind it has; `remove` takes as many as there are.
        for &kind in CounterKind::ALL {
            remove(run.game(), object, kind, u8::MAX)?;
        }
    }
    let after = counters_on(run, permanents, None);
    Ok(Outcome::of(permanents.slots(), after.abs_diff(before)))
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
    let before = counters_on(run, permanents, Some(kind));
    let mut left = permanents;
    while let Some(object) = run.state().next_permanent(&mut left) {
        change(run.game(), object, kind, count)?;
    }
    let after = counters_on(run, permanents, Some(kind));
    Ok(Outcome::of(permanents.slots(), after.abs_diff(before)))
}

/// The counters of `kind` on `permanents`, or of every kind with none.
fn counters_on<L: Layout>(
    run: &Run<'_, '_, L>,
    permanents: Permanents<L>,
    kind: Option<CounterKind>,
) -> u32 {
    let s = run.state();
    permanents.slots().sum(|slot| match kind {
        Some(kind) => u32::from(s.counters_of(slot, kind)),
        None => s.counters(slot).iter().map(|(_, n)| u32::from(n)).sum(),
    })
}
