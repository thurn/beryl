//! Delayed triggered abilities
//! ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)), and the reflexive ones
//! that follow their rules
//! ([CR 603.12](https://yawgatog.com/resources/magic-rules/#R60312)).
//!
//! Implements: CR 603.7, CR 603.12

use beryl_card::{Amount, Condition, Objects};

use crate::card::{AbilityIx, TriggerEvent};
use crate::filter::is_for;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::resolution::Resolution;
use crate::stack::trigger;
use crate::state::{
    AbilityRef, ObjectRef, PendingTrigger, State, TriggerInfo, TriggeredAbility, TurnStep, Zone,
};
use crate::table::GameTable;

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
    let delayed = PendingTrigger {
        of: source(resolution, ability),
        info: TriggerInfo {
            amount,
            object,
            ..TriggerInfo::default()
        },
    };
    game.state.delay_trigger(delayed)
}

/// The delayed ability and its source as the spell or ability that creates it resolves
/// (see CR 603.7d and CR 603.7e).
pub(crate) fn source<L: Layout>(
    resolution: &Resolution<'_, L>,
    ability: AbilityIx,
) -> AbilityRef<L, TriggeredAbility> {
    let entry = &resolution.entry;
    AbilityRef {
        source: entry.object(),
        card: entry.card(),
        face: entry.face(),
        ability: TriggeredAbility::own(ability),
        controller: entry.controller().into(),
    }
}

/// Drops unfired abilities whose stated turn has ended.
/// Under [CR 603.7b]: "A delayed triggered ability will trigger only once—the next time its
/// trigger event occurs—unless it has a stated duration, such as “this turn.”"
///
/// [CR 603.7b]: https://yawgatog.com/resources/magic-rules/#R6037b
pub(crate) fn end_turn<L: Layout>(s: &mut State<L>, t: &GameTable) {
    s.retain_delayed(|d| {
        !trigger::ability(t, &d.of)
            .trigger()
            .is_some_and(|trigger| trigger.exact_turn)
    });
}

/// [`Effect::Delay`](beryl_card::Effect::Delay): "[When], [effect]", said by a resolving
/// spell or ability
/// ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a)): "Delayed triggered
/// abilities are created during the resolution of spells or abilities". The number it
/// carries is settled now
/// (see [CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "the answer is
/// determined only once, when the effect is applied." So is the object it carries: the
/// first of those `object` names, in the order the game lists them, as it is now. When that
/// is what an instruction before moved, it is the new object it became
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object that moves
/// from one zone to another becomes a new object". It acts on nothing, so its outcome is
/// nothing.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    ability: AbilityIx,
    carrying: Option<Amount>,
    object: Option<Objects>,
) -> Result<Outcome<L>, Full> {
    let amount = carrying.map_or(Ok(0), |carrying| run.scene().amount(carrying))?;
    let object = if let Some(objects) = object {
        run.in_game_order(run.scene().objects(objects))?
            .as_slice()
            .first()
            .map(|&slot| run.state().object_ref(slot))
    } else {
        None
    };
    let (game, resolution) = run.resolving();
    create(game, resolution, ability, amount, object)?;
    Ok(Outcome::NOTHING)
}

/// [`Effect::Reflexive`](beryl_card::Effect::Reflexive): "When [something happens] this way,
/// [effect]", said by a resolving spell or ability: a reflexive triggered ability
/// ([CR 603.12]): "These reflexive triggered abilities follow the rules for delayed triggered
/// abilities (see rule 603.7), except that they're checked immediately after being created and
/// trigger based on whether the trigger event or events occurred earlier during the resolution
/// of the spell or ability that created them." So its condition is read now, of what the
/// instructions before did, and if it holds the ability waits at once with the other triggers,
/// with the controller and source a delayed ability has (CR 603.7d, CR 603.7e). It is put on
/// the stack the next time a player would receive priority and chooses its targets then, and
/// one with no legal target is removed from the stack (see [CR 603.3d]): "If a choice is required
/// when the triggered ability goes on the stack but no legal choices can be made for it ...
/// the ability is simply removed from the stack." It acts on nothing, so its outcome is
/// nothing.
///
/// [CR 603.12]: https://yawgatog.com/resources/magic-rules/#R60312
/// [CR 603.3d]: https://yawgatog.com/resources/magic-rules/#R6033d
#[cold]
#[inline(never)]
pub(crate) fn reflexive<L: Layout>(
    run: &mut Run<'_, '_, L>,
    condition: Option<Condition>,
    ability: AbilityIx,
) -> Result<Outcome<L>, Full> {
    if condition.is_none_or(|condition| run.scene().holds(&condition)) {
        let (game, resolution) = run.resolving();
        let reflexive = PendingTrigger {
            of: source(resolution, ability),
            info: TriggerInfo::default(),
        };
        game.state.queue_trigger(reflexive)?;
    }
    Ok(Outcome::NOTHING)
}

/// Fires the delayed triggers that wait for their own source to leave the battlefield, of the
/// permanents of one event about to leave it: each once ([CR 603.7b]): "A delayed triggered
/// ability will trigger only once—the next time its trigger event occurs". It keeps the object
/// it carries.
///
/// [CR 603.7b]: https://yawgatog.com/resources/magic-rules/#R6037b
#[cold]
#[inline(never)]
pub(crate) fn fire_on_leaving<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    leaving: impl Iterator<Item = (L::Slot, Zone)>,
) -> Result<(), Full> {
    for (left, _) in leaving {
        let object = s.object_ref(left);
        s.fire_delayed(|d| {
            d.of.source == Some(object)
                && trigger::ability(t, &d.of)
                    .trigger()
                    .is_some_and(|trigger| trigger.is_this(TriggerEvent::Leaves))
        })?;
    }
    Ok(())
}

/// Fires the delayed triggers waiting for this step, each once (see [CR 603.7b]).
/// Each step occurrence has its own beginning event. A trigger created during one
/// occurrence can fire at the next occurrence of the same step.
///
/// [CR 603.7b]: https://yawgatog.com/resources/magic-rules/#R6037b
pub(crate) fn fire_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    step: TurnStep,
    active: Seat,
) -> Result<(), Full> {
    s.fire_delayed(|d| match trigger::ability(t, &d.of).trigger() {
        Some(trigger) if trigger.delayed => {
            trigger::begins(trigger.event, step, || false)
                && is_for(&trigger.whose, active, d.of.controller.seat())
        }
        _ => unreachable!("a delayed trigger's ability is a delayed one"),
    })
}
