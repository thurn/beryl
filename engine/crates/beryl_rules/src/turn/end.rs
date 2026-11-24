//! Ending the turn during resolution.
//!
//! Implements: CR 725.1

use crate::card::Flow;
use crate::continuous::arrival_timestamps;
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::Layout;
use crate::replacement::event::{self, Cause, Entry, Reason};
use crate::resolution::Resolution;
use crate::state::TurnStep;
use crate::state::{Pending, Zone};
use crate::{combat, sba};

/// Ends the turn as part of the current resolution. If state-based actions need a
/// choice, the resolution replays with that answer before it can enter cleanup.
///
/// Under [CR 725.1a], "those abilities cease to exist." This applies to triggers waiting when
/// the process starts, so triggers from the exile and state-based actions can still wait
/// for cleanup ([CR 725.1f]: "those abilities are put onto the stack during the cleanup step").
///
/// [CR 725.1a]: https://yawgatog.com/resources/magic-rules/#R7251a
/// [CR 725.1f]: https://yawgatog.com/resources/magic-rules/#R7251f
#[cold]
pub(crate) fn end_the_turn<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
) -> Flow {
    game.state.discard_waiting_triggers();
    exile_stack(game)?;
    game.sink.enter(|_| Cause::Rules);
    let result = cleanup(game, resolution);
    game.sink.leave();
    result
}

/// [CR 725.1b] says to "Exile every object on the stack, including the object that’s resolving."
/// The stack zone still holds the resolving spell's object even though the replay
/// transaction has taken its entry. Abilities have only stack entries, so removing
/// those entries is enough.
///
/// [CR 725.1b]: https://yawgatog.com/resources/magic-rules/#R7251b
fn exile_stack<L: Layout>(game: &mut Game<'_, L>) -> Result<(), Full> {
    let s = &mut *game.state;
    let entries = s.stack().len();
    while s.take_top_of_stack().is_some() {}
    let spells = s.in_zone(Zone::Stack);
    if !spells.is_empty() {
        event::move_batch(
            s,
            game.table,
            game.sink,
            spells,
            Zone::Exile,
            Reason::Exile,
            Entry::default(),
        )?;
    }
    for place in (0..entries).rev() {
        game.sink.departed(|| place);
    }
    game.sink.resolved();
    s.settle_captured_words();
    Ok(())
}

/// Under [CR 725.1c], "Check state-based actions. No player gets priority, and no triggered
/// abilities are put onto the stack." When the checks finish, [CR 725.1d] says to
/// "remove all creatures and planeswalkers from combat" and skip to cleanup. This
/// skips any end step still ahead and repeats cleanup if it is already in progress.
///
/// [CR 725.1c]: https://yawgatog.com/resources/magic-rules/#R7251c
/// [CR 725.1d]: https://yawgatog.com/resources/magic-rules/#R7251d
fn cleanup<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow {
    loop {
        // A state-based action can return multiple permanents whose timestamp order matters.
        while let Pending::OrderEntered { group } = *game.state.pending() {
            let decision = arrival_timestamps::decision(game.state, group);
            let chosen = resolution.answer(game, &decision)?;
            let permanent = decision
                .candidates
                .object(chosen)
                .expect("a permanent to order");
            arrival_timestamps::pick(game.state, group, permanent)?;
        }
        if !sba::check(game.state, game.table, game.sink)? {
            break;
        }
        if game.state.outcome().is_some() {
            return Ok(());
        }
        if let Pending::LegendRule { mut kept } = *game.state.pending() {
            while let Some(decision) = sba::legend_decision(game.state, game.table, kept) {
                let chosen = resolution.answer(game, &decision)?;
                kept.insert(
                    decision
                        .candidates
                        .object(chosen)
                        .expect("a legend to keep"),
                );
            }
            sba::apply_legend_rule(game.state, game.table, game.sink, kept)?;
        }
    }

    let s = &mut *game.state;
    s.forget_last_known();
    if combat::in_combat(s.turn().step()) {
        combat::end_combat(s);
    }
    if !s.has_turn_plan() {
        s.plan_from_current_step(game.table.format())?;
    }
    game.sink.note(s.cut_to_cleanup()?);
    s.empty_mana_pools();
    let step = s.take_planned_step().expect("the cleanup step");
    debug_assert_eq!(step, TurnStep::Cleanup);
    super::begin_step(s, game.table, game.sink, step)?;
    Ok(())
}
