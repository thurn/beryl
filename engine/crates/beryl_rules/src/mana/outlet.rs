//! Outlet mana abilities activated at priority (see [CR 605.3b]): mana abilities whose cost
//! sacrifices or taps a chosen permanent on the battlefield, with or without `{T}` and with no
//! mana cost.
//!
//! Implements: none. Offers and carries out outlet activations of CR 605.3.
//!
//! [CR 605.3b]: https://yawgatog.com/resources/magic-rules/#R6053b

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, Chosen, ChosenKind, CostAction, ManaColor, ObjectAbility, Prompt,
};
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::replacement::event::{self, Sink};
use crate::state::{AbilityRef, Pending, PendingTrigger, State, TriggeredAbility};
use crate::table::{Capability, GameTable, outlet_part};
use beryl_kernel::chosen::ChosenValue;
use beryl_kernel::reference::Permanent;

use super::KINDS;
use super::solver::lowest_kind;

/// The permanents `player` may choose to pay `a`'s outlet cost part: those
/// [`crate::casting::parts::candidates`] accepts, excluding `slot` itself when the cost both
/// taps the source (`{T}`) and taps a chosen permanent (a permanent cannot be tapped twice).
pub(super) fn outlet_candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    a: &Ability,
    action: CostAction,
    chosen: &Chosen,
) -> Bitboard<L::Slot> {
    let mut able = crate::casting::parts::candidates(s, t, player, slot, action, chosen);
    if action == CostAction::Tap && a.cost().does(CostAction::Tap) {
        able.remove(slot);
    }
    able
}

/// Begins activating an outlet mana ability at priority: takes the single candidate without
/// asking when only one equivalence class is eligible, else asks [`Pending::Outlet`].
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn begin_outlet<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    ability: ObjectAbility,
    printed: &'static Ability,
    action: CostAction,
    chosen: &Chosen,
) -> Result<(), Full> {
    let slot = permanent.slot();
    let player = s.controller(slot);
    let able = outlet_candidates(s, t, player, slot, printed, action, chosen);
    let offered = crate::engine::collapse(s, able);
    if offered.count() == 1 {
        let paid = offered.min(|s| s).expect("one eligible permanent");
        return finish_outlet_pick(s, t, sink, permanent, ability, printed, paid);
    }
    s.ask(Pending::Outlet {
        slot,
        ability,
        paid: None,
    });
    Ok(())
}

/// Completes the permanent choice for an outlet: activates at once when the ability produces
/// one kind of mana, or opens [`Pending::Outlet`] with `paid: Some` to choose the color.
fn finish_outlet_pick<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    ability: ObjectAbility,
    printed: &'static Ability,
    paid: L::Slot,
) -> Result<(), Full> {
    let kinds = printed.produces().colors;
    if kinds.is_power_of_two() {
        let color = ManaColor::ALL[lowest_kind(kinds)];
        return activate_outlet(s, t, sink, permanent, ability, printed, paid, color);
    }
    s.ask(Pending::Outlet {
        slot: permanent.slot(),
        ability,
        paid: Some(paid),
    });
    Ok(())
}

/// The decision for [`Pending::Outlet`]: either choosing which permanent pays (`paid: None`)
/// or choosing which color of mana the ability adds (`paid: Some`).
#[cold]
#[inline(never)]
pub(crate) fn outlet_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    ability: ObjectAbility,
    paid: Option<L::Slot>,
) -> Result<Decision<L>, Full> {
    let player = s.controller(slot);
    if paid.is_some() {
        return crate::interpret::catalog::decision(
            t,
            player,
            ChosenKind::Color,
            Bitboard::single(slot),
        );
    }
    let printed = s
        .abilities(t, slot)
        .find(|&(a, _)| a == ability)
        .map(|(_, a)| a)
        .expect("the outlet ability asked about");
    let (action, chosen) = outlet_part(printed).expect("an outlet mana ability");
    let able = outlet_candidates(s, t, player, slot, printed, action, &chosen);
    let offered = crate::engine::collapse(s, able);
    let purpose = crate::casting::parts::purpose(action, &chosen);
    Ok(Decision {
        player,
        decider: player,
        kind: DecisionKind::ChooseObject,
        candidates: Candidates::Objects(offered),
        prompt: Prompt::new(purpose).most(1),
        about: Bitboard::EMPTY,
        filter: None,
    })
}

/// Applies the answer to [`Pending::Outlet`].
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn answer_outlet<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    ability: ObjectAbility,
    paid: Option<L::Slot>,
    option: Option<Opt<L>>,
    object: Option<L::Slot>,
) -> Result<(), Full> {
    let slot = permanent.slot();
    let printed = s
        .abilities(t, slot)
        .find(|&(a, _)| a == ability)
        .map(|(_, a)| a)
        .expect("the outlet ability asked about");
    match paid {
        None => {
            let chosen_slot = object.expect("a permanent chosen for the outlet");
            finish_outlet_pick(s, t, sink, permanent, ability, printed, chosen_slot)
        }
        Some(paid_slot) => {
            let ChosenValue::Color(colors) =
                crate::interpret::catalog::value(ChosenKind::Color, option)
            else {
                unreachable!("a color among a color's candidates");
            };
            let color = ManaColor::ALL[lowest_kind(colors.0)];
            activate_outlet(s, t, sink, permanent, ability, printed, paid_slot, color)
        }
    }
}

/// Activates the outlet mana ability `ability` of the permanent in `slot`, its cost taking the
/// permanent in `paid` and its mana of `color`, in the middle of a payment (see CR 605.3a): the
/// payment's steps chose both.
#[cold]
#[inline(never)]
pub(crate) fn activate_paid<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    ability: ObjectAbility,
    paid: L::Slot,
    color: ManaColor,
) -> Result<(), Full> {
    let permanent = s.permanent_at(slot).expect("an outlet on the battlefield");
    let printed = s
        .abilities(t, slot)
        .find(|&(a, _)| a == ability)
        .map(|(_, a)| a)
        .expect("the outlet ability chosen");
    activate_outlet(s, t, sink, permanent, ability, printed, paid, color)
}

/// Pays an outlet mana ability's cost and adds its mana in one event group (see CR 605.3b).
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn activate_outlet<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    ability: ObjectAbility,
    printed: &'static Ability,
    paid: L::Slot,
    color: ManaColor,
) -> Result<(), Full> {
    sink.together();
    let done = activate_outlet_whole(s, t, sink, permanent, ability, printed, paid, color);
    sink.apart();
    done
}

#[allow(clippy::too_many_arguments)]
fn activate_outlet_whole<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    ability: ObjectAbility,
    printed: &'static Ability,
    paid: L::Slot,
    color: ManaColor,
) -> Result<(), Full> {
    let slot = permanent.slot();
    let controller = s.controller(slot);
    let source = permanent.into();
    let (card, face) = s.definition(slot).record(t);
    let (action, chosen) = outlet_part(printed).expect("an outlet mana ability");
    let tapped = printed.cost().does(CostAction::Tap);
    let has_changes = tapped && t.can(Capability::ManaChanges) && s.has_mana_changes();
    let extra = if has_changes {
        super::changes::matching(s, t, slot)?.add_for_single_color(color)?
    } else {
        [0; KINDS]
    };
    if tapped {
        event::set_tapped(s, t, sink, permanent, true)?;
    }
    match action {
        CostAction::Sacrifice => {
            crate::casting::parts::pay_with_all(
                s,
                t,
                sink,
                action,
                &chosen,
                controller,
                Bitboard::single(paid),
            )?;
        }
        CostAction::Tap => {
            if let Some(target) = s.permanent_at(paid) {
                event::set_tapped(s, t, sink, target, true)?;
            }
        }
        CostAction::Discard
        | CostAction::Exile
        | CostAction::Reveal
        | CostAction::RemoveCounter(_) => {
            unreachable!("an outlet cost only sacrifices or taps a permanent")
        }
    }
    let index = match ability {
        ObjectAbility::Own(i) | ObjectAbility::Merged(_, i) => i,
        ObjectAbility::Granted(_) | ObjectAbility::Lent(..) => AbilityIx::new(0),
    };
    let activation = || PendingTrigger {
        of: AbilityRef {
            source: Some(source),
            controller: controller.into(),
            ability: TriggeredAbility::own(index),
            card,
            face,
        },
        ..PendingTrigger::default()
    };
    crate::interpret::mana_ability(s, t, sink, activation, printed, color)?;
    if has_changes {
        super::changes::add_extra(s, sink, permanent, controller, extra)?;
    }
    Ok(())
}
