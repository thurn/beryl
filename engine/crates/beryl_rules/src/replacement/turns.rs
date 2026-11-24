//! Replacing the start of a turn with skipping it.
//!
//! Implements: CR 614.10

use beryl_card::prompt::Purpose;
use beryl_card::{Prompt, Replaced, Replacement, Rewrite};

use super::{may_replace, statics};
use crate::decision::{Decision, DecisionKind, Opt};
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::replacement::event::ReplacementKind;
use crate::state::State;
use crate::table::GameTable;

/// The affected player can take the turn or pick one of their tapped permanents whose
/// ability replaces the turn. See [CR 616.1]: "the affected player chooses one to apply".
/// Under [CR 614.10]: "Once a step, phase, or turn has started, it can no longer be skipped".
///
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 614.10]: https://yawgatog.com/resources/magic-rules/#R61410
pub(crate) fn choose<L: Layout>(s: &State<L>, t: &GameTable) -> Result<Option<Decision<L>>, Full> {
    if !s
        .replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::BeginTurn))
    {
        return Ok(None);
    }
    let player = s.next_turn_player();
    let mut options = <L::Queue<Opt<L>, { crate::decision::OPTIONS }>>::default();
    options.push(Opt::Done)?;
    for &slot in s.battlefield() {
        if s.controller(slot) == player
            && s.object(slot).is_tapped()
            && may_replace(s, t, slot, ReplacementKind::BeginTurn)
            && let Some((_, ability, ())) = statics(s, t, slot, |_, replacement| {
                matches!(
                    replacement,
                    Replacement {
                        replaces: Replaced::BeginTurn,
                        rewrite: Rewrite::MaySkipTurnToUntap,
                    }
                )
                .then_some(())
            })
            .next()
        {
            crate::coverage::hit(ability);
            options.push(Opt::Object(slot))?;
        }
    }
    Ok((options.len() > 1).then(|| {
        Decision::options(
            player,
            DecisionKind::ChooseUpTo,
            options,
            Prompt::new(Purpose::Replace),
        )
    }))
}
