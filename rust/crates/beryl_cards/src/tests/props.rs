//! What a card's scenarios may name (plan, Verification): their own card, its tokens, basic
//! lands and the designated props ([`crate::props`]), so one card's change cannot break another
//! card's tests. A scenario about two cards in particular says so with
//! `Scenario::interaction`.

use std::collections::BTreeSet;

use beryl_engine::card::{CardDef, Supertypes};
use beryl_scenario::Scenario;
use serde_json::Value;

use crate::{CARDS, SCENARIOS, props};

/// `def`'s name, its faces' names and those of the tokens it makes.
fn names_of(def: &'static CardDef, into: &mut BTreeSet<&'static str>) {
    into.insert(def.name);
    into.extend(
        def.faces
            .iter()
            .map(|face| face.name)
            .filter(|n| !n.is_empty()),
    );
    for &token in def.tokens {
        if !into.contains(token.name) {
            names_of(token, into);
        }
    }
}

fn card(name: &str) -> &'static CardDef {
    CARDS
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not a card"))
}

/// Every card name any card's scenario could write: the cards, their faces and their tokens.
fn every_name() -> BTreeSet<&'static str> {
    let mut all = BTreeSet::new();
    for &def in CARDS.cards() {
        names_of(def, &mut all);
    }
    all
}

/// The card names `scenario` writes anywhere: in its setup, its actions or its assertions.
fn named(scenario: &Scenario, known: &BTreeSet<&'static str>) -> BTreeSet<String> {
    fn walk(value: &Value, known: &BTreeSet<&'static str>, into: &mut BTreeSet<String>) {
        match value {
            Value::String(s) if known.contains(s.as_str()) => {
                into.insert(s.clone());
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, known, into)),
            Value::Object(fields) => fields.values().for_each(|v| walk(v, known, into)),
            _ => {}
        }
    }
    let json = serde_json::to_value(scenario).expect("a scenario serializes");
    let mut into = BTreeSet::new();
    walk(&json["setup"], known, &mut into);
    walk(&json["actions"], known, &mut into);
    into
}

/// What every scenario may name: basic lands and the props, with their faces and tokens.
fn shared() -> BTreeSet<&'static str> {
    let mut allowed = BTreeSet::new();
    for &def in CARDS.cards() {
        if def.front().supertypes.contains(Supertypes::BASIC) {
            names_of(def, &mut allowed);
        }
    }
    for &prop in props::ALL {
        names_of(card(prop), &mut allowed);
    }
    allowed
}

#[test]
fn scenarios_name_only_their_card_its_tokens_basic_lands_and_props() {
    let known = every_name();
    let shared = shared();
    let mut failures = Vec::new();
    for (name, scenarios) in SCENARIOS.iter().copied().chain(crate::library::by_card()) {
        let mut own = shared.clone();
        names_of(card(name), &mut own);
        for scenario in scenarios {
            let mut allowed = own.clone();
            for &other in scenario.interaction {
                names_of(card(other), &mut allowed);
            }
            let named = named(scenario, &known);
            let strays: Vec<_> = named
                .iter()
                .filter(|n| !allowed.contains(n.as_str()))
                .collect();
            if !strays.is_empty() {
                failures.push(format!("{name}: \"{}\" names {strays:?}", scenario.name));
            }
            for &other in scenario.interaction {
                if !named.contains(other) {
                    failures.push(format!(
                        "{name}: \"{}\" is marked as an interaction with {other}, which it \
                         never names",
                        scenario.name
                    ));
                }
                if own.contains(other) {
                    failures.push(format!(
                        "{name}: \"{}\" marks {other} as an interaction, which it may name \
                         without",
                        scenario.name
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
