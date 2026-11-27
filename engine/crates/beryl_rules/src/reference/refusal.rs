//! Agreement between the decisions the engine offers and the refusal reasons it reports for
//! cards the client should reject on click ([`crate::refusal`], [`crate::attack_refusal`]).
//!
//! No card offered in a priority decision may be refused at that priority, and no creature
//! offered as an attacker may be refused as an attacker. Because `priority_decision` and
//! `attack_decision` collapse interchangeable objects to one representative, every card in the
//! same equivalence class is checked against its representative's options.
//!
//! Implements: none. Checks that refusal queries agree with the options the engine offers.

use beryl_kernel::decision::interchangeable;

use crate::combat;
use crate::decision::{Candidates, Opt};
use crate::engine::{priority_decision, refusal};
use crate::ids::SlotId;
use crate::invariants::{At, Violation};
use crate::layout::{Layout, Queue};
use crate::observation::Observation;
use crate::stack::trigger;
use crate::state::{Pending, State};
use crate::table::GameTable;
use crate::turn;

use super::check;

/// Checks that no card offered in the open priority or declare-attackers decision is also
/// reported as refused.
pub(super) fn check_refusals<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    if let Pending::None = *s.pending() {
        if trigger::next_to_stack(s).is_none() {
            return check_priority_refusals(s, t);
        }
    } else if let Pending::DeclareAttackers { attacker: None } = *s.pending() {
        return check_attack_refusals(s, t);
    }
    Ok(())
}

/// Checks that every card in the priority holder's hand (or exiled as a prepare spell copy)
/// that has an option in `priority_decision` is not refused by [`refusal`].
fn check_priority_refusals<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    let player = s.turn().priority();
    let cards = turn::hand(s, player) | s.prepare_spells_for(player);
    if cards.is_empty() {
        return Ok(());
    }
    let Ok(decision) = priority_decision(s, t) else {
        return Ok(());
    };
    let Candidates::Options(opts) = decision.candidates else {
        return Ok(());
    };

    let mut offered = Vec::new();
    for &opt in opts.as_slice() {
        match opt {
            Opt::PlayLand(slot)
            | Opt::Cast { slot, .. }
            | Opt::Activate { slot, .. }
            | Opt::Mana { slot, .. }
            | Opt::TurnFaceUp(slot) => offered.push(slot),
            Opt::Pass
            | Opt::Play
            | Opt::Draw
            | Opt::Keep
            | Opt::Mulligan
            | Opt::FreeMulligan
            | Opt::Trigger(_)
            | Opt::Yes
            | Opt::No
            | Opt::Done
            | Opt::Attack { .. }
            | Opt::AttackAll(_)
            | Opt::Defender(_)
            | Opt::Block(_)
            | Opt::Lethal(_)
            | Opt::Pool(_)
            | Opt::Number(_)
            | Opt::Plan(_)
            | Opt::Player(_)
            | Opt::Ability(_)
            | Opt::Object(_)
            | Opt::Companion => {}
        }
    }
    if offered.is_empty() {
        return Ok(());
    }

    let obs = Observation::new(s, player);
    let refused_offered = cards.filter(|slot| {
        let is_offered = offered
            .iter()
            .any(|&cand| cand == slot || interchangeable(s, cand, slot));
        is_offered && refusal(obs, t, slot).is_some()
    });
    if let Some(index) = refused_offered.min(SlotId::index) {
        return check(
            false,
            "a card offered at priority was also refused",
            Some(At::Slot(index)),
        );
    }
    Ok(())
}

/// Checks that every creature offered in `combat::attack_decision` is not refused by
/// [`combat::attack_refusal`].
fn check_attack_refusals<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    let Ok(decision) = combat::attack_decision(s, t, None) else {
        return Ok(());
    };
    let Candidates::Options(opts) = decision.candidates else {
        return Ok(());
    };

    let mut offered = Vec::new();
    for &opt in opts.as_slice() {
        match opt {
            Opt::Attack { slot, .. } | Opt::AttackAll(slot) => offered.push(slot),
            Opt::Pass
            | Opt::Play
            | Opt::Draw
            | Opt::Keep
            | Opt::Mulligan
            | Opt::FreeMulligan
            | Opt::Trigger(_)
            | Opt::Yes
            | Opt::No
            | Opt::Done
            | Opt::PlayLand(_)
            | Opt::Cast { .. }
            | Opt::Activate { .. }
            | Opt::Mana { .. }
            | Opt::TurnFaceUp(_)
            | Opt::Defender(_)
            | Opt::Block(_)
            | Opt::Lethal(_)
            | Opt::Pool(_)
            | Opt::Number(_)
            | Opt::Plan(_)
            | Opt::Player(_)
            | Opt::Ability(_)
            | Opt::Object(_)
            | Opt::Companion => {}
        }
    }
    if offered.is_empty() {
        return Ok(());
    }

    let active = s.turn().active();
    let obs = Observation::new(s, active);
    for (slot, derived) in s.permanents_with_derived() {
        if derived.controller != active {
            continue;
        }
        let is_offered = offered
            .iter()
            .any(|&cand| cand == slot || interchangeable(s, cand, slot));
        if is_offered {
            check(
                combat::attack_refusal(obs, t, slot).is_none(),
                "a creature offered as an attacker was also refused",
                Some(At::Slot(slot.index())),
            )?;
        }
    }
    Ok(())
}
