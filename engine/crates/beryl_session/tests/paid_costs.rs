//! What a spell on the stack says of the costs it was cast with, which its printed text does
//! not: an overloaded Damn destroys each creature, not its target
//! ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)): "If you chose to pay
//! this spell's overload cost, change its text by replacing all instances of the word 'target'
//! with the word 'each.'"

use crate::common::fixtures::{DAMN_NORMAL_CAST, DAMN_OVERLOADED};
use beryl_cards::CARDS;
use beryl_engine::{Observation, Seat};
use beryl_session::StopsSetting;
use beryl_session::protocol::{Moment, PaidCost, StackItem, player_view};

/// The costs each player sees on the stack after Damn is cast.
fn paid(scenario: &beryl_scenario::Scenario) -> Vec<Vec<PaidCost>> {
    let mut seen = Vec::new();
    beryl_scenario::visit(&CARDS, scenario, |t, s, _| {
        for viewer in Seat::BOTH {
            let view = player_view(
                t,
                Observation::new(s, viewer),
                Moment::Thinking {
                    resolving: None,
                    decision: None,
                },
                StopsSetting::SMART,
                &[],
            );
            if let Some(StackItem::Spell { paid, .. }) = view.stack.last() {
                seen.push(paid.clone());
            }
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    seen
}

#[test]
fn an_overloaded_spell_on_the_stack_says_so_to_both_players() {
    let seen = paid(DAMN_OVERLOADED);
    assert!(!seen.is_empty(), "Damn is seen on the stack");
    assert!(seen.iter().all(|p| p == &[PaidCost::Overload]), "{seen:?}");
}

#[test]
fn a_spell_cast_for_its_mana_cost_names_no_cost() {
    let seen = paid(DAMN_NORMAL_CAST);
    assert!(!seen.is_empty(), "Damn is seen on the stack");
    assert!(seen.iter().all(Vec::is_empty), "{seen:?}");
}
