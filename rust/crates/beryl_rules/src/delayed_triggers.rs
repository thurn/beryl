//! Delayed triggered abilities
//! ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).

use crate::card::AbilityIx;
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::{PendingTrigger, TriggerInfo};
use crate::trigger;

/// What is resolving creates a delayed triggered ability: the ability at `ability` on its
/// card, triggering once at the step its delayed [`crate::card::Trigger`] names, controlled
/// by the controller of what is resolving, carrying `amount`
/// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
pub fn create<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &Resolution<'_, L>,
    ability: AbilityIx,
    amount: u8,
) -> Result<(), Full> {
    let entry = &resolution.entry;
    let delayed = PendingTrigger {
        source: entry.object,
        card: entry.card,
        face: entry.face,
        ability,
        controller: entry.controller,
        info: TriggerInfo {
            amount,
            ..TriggerInfo::default()
        },
    };
    trigger::delay(game.state, delayed)
}
