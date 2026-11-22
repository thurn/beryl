//! Instructions that add phases to the current turn.
//!
//! Implements: none. Uses the kernel's phase insertion.

use beryl_card::TurnPhase;
use beryl_kernel::TurnChange;

use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement::event::Event;

/// Adds the named phases after the current phase (see [CR 500.8]): "They do this by adding
/// the phases directly after the specified phase." If the current phase does not match the
/// instruction's anchor, there is no specified phase to add them after.
///
/// [CR 500.8]: https://yawgatog.com/resources/magic-rules/#R5008
pub(crate) fn additional<L: Layout>(
    run: &mut Run<'_, '_, L>,
    after: TurnPhase,
    phases: &[TurnPhase],
) -> Result<Outcome<L>, Full> {
    let (game, _) = run.resolving();
    if !game.state.has_turn_plan() {
        if phases.is_empty() || TurnPhase::of(game.state.turn().step()) != after {
            return Ok(Outcome::NOTHING);
        }
        game.state.plan_from_current_step(game.table.format())?;
    }
    if !game.state.insert_phases_after_current(after, phases)? {
        return Ok(Outcome::NOTHING);
    }
    for &phase in phases {
        game.sink
            .note(Event::TurnPlanChanged(TurnChange::InsertPhase(phase)));
    }
    Ok(Outcome::of(
        Bitboard::EMPTY,
        u32::try_from(phases.len()).expect("the turn plan fits in a u32"),
    ))
}
