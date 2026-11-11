//! What the target being announced is chosen for, said without running the effect: a player
//! is told before they aim at what is their own.

use crate::cast::entry_ability;
use crate::layout::Layout;
use crate::state::{CastStep, Pending, State, Zone};
use crate::table::GameTable;

/// Whether the effect whose target is being announced
/// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "The player announces
/// their choice of an appropriate object or player for each target the spell requires") would
/// take the object in `slot` from its controller, were it chosen: a permanent removed from
/// the battlefield ([`Ability::removes_target`](crate::card::Ability::removes_target)), or a
/// spell countered
/// ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a): "To counter a spell or
/// ability means to cancel it, removing it from the stack. It doesn't resolve and none of
/// its effects occur. A countered spell is put into its owner's graveyard."). This describes
/// the choice: it neither runs the effect nor says whether the object is a legal target.
pub fn target_takes<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let Pending::Casting(c) = *s.pending() else {
        return false;
    };
    let announcing = matches!(c.step, CastStep::TargetCount | CastStep::Target);
    let ability = s.stack().last().and_then(|entry| entry_ability(t, entry));
    ability
        .filter(|_| announcing)
        .is_some_and(|ability| match s.object(slot).zone() {
            Zone::Battlefield => ability.removes_target(c.spec),
            Zone::Stack => ability.counters_target(c.spec),
            _ => false,
        })
}
