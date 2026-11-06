//! What a spell on the stack says of the costs it was cast with, which its printed text does
//! not: an overloaded Damn destroys each creature, not its target
//! ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)): "If you chose to pay
//! this spell's overload cost, change its text by replacing all instances of the word 'target'
//! with the word 'each.'"

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::{Observation, Seat};
use beryl_session::StopsSetting;
use beryl_session::protocol::{Moment, PaidCost, StackItem, player_view};

/// The costs Damn was cast with in its scenario named `name`, as each player sees its spell
/// on the stack.
fn paid(name: &str) -> Vec<Vec<PaidCost>> {
    let (_, scenarios) = SCENARIOS
        .iter()
        .find(|&&(card, _)| card == names::DAMN)
        .expect("the card's scenarios");
    let scenario = scenarios
        .iter()
        .find(|s| s.name == name)
        .expect("the scenario");
    let mut seen = Vec::new();
    beryl_scenario::visit(&CARDS, scenario, |t, s, _| {
        for viewer in Seat::BOTH {
            let view = player_view(
                t,
                Observation::new(s, viewer),
                Moment::Thinking,
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
    let seen = paid("Damn overloaded destroys each creature, targeting none");
    assert!(!seen.is_empty(), "Damn is seen on the stack");
    assert!(seen.iter().all(|p| p == &[PaidCost::Overload]), "{seen:?}");
}

#[test]
fn a_spell_cast_for_its_mana_cost_names_no_cost() {
    let seen = paid("Damn for {B}{B} destroys its target alone");
    assert!(!seen.is_empty(), "Damn is seen on the stack");
    assert!(seen.iter().all(Vec::is_empty), "{seen:?}");
}
