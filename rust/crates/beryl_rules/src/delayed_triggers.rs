//! Delayed triggered abilities
//! ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).

use beryl_card::{Amount, Objects};

use crate::card::AbilityIx;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::{AbilityRef, ObjectRef, PendingTrigger, TriggerInfo};
use crate::trigger;

/// What is resolving creates a delayed triggered ability: the ability at `ability` on its
/// card, triggering once at the step its delayed [`crate::card::Trigger`] names, controlled
/// by the controller of what is resolving, carrying `amount` and `object`
/// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
pub fn create<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &Resolution<'_, L>,
    ability: AbilityIx,
    amount: u8,
    object: Option<ObjectRef<L>>,
) -> Result<(), Full> {
    let entry = &resolution.entry;
    let delayed = PendingTrigger {
        of: AbilityRef {
            source: entry.object(),
            card: entry.card(),
            face: entry.face(),
            ability,
            controller: entry.controller(),
        },
        info: TriggerInfo {
            amount,
            object,
            ..TriggerInfo::default()
        },
    };
    trigger::delay(game.state, delayed)
}

/// [`Effect::Delay`](beryl_card::Effect::Delay): "[When], [effect]", said by a resolving
/// spell or ability
/// ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a)): "Delayed triggered
/// abilities are created during the resolution of spells or abilities". The number it
/// carries is settled now
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "the answer is
/// determined only once, when the effect is applied." So is the object it carries: the
/// first of those `object` names, in the order the game lists them, as it is now. When that
/// is what an instruction before moved, it is the new object it became
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object that moves
/// from one zone to another becomes a new object". It acts on nothing, so its outcome is
/// nothing.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    ability: AbilityIx,
    carrying: Amount,
    object: Option<Objects>,
) -> Result<Outcome<L>, Full> {
    let amount = run.scene().amount(carrying);
    let object = object.and_then(|object| {
        let s = run.state();
        let slot = s.in_candidate_order(run.scene().objects(object)).next()?;
        Some(s.object_ref(slot))
    });
    let (game, resolution) = run.resolving();
    create(game, resolution, ability, amount, object)?;
    Ok(Outcome::NOTHING)
}
