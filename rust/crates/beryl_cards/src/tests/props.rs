//! What a card's scenarios may name: their own card, its tokens, basic
//! lands and the designated props ([`crate::props`]), so one card's change cannot break another
//! card's tests. A scenario about two cards in particular says so with
//! `Scenario::interaction`.

use std::collections::BTreeSet;

use beryl_engine::card::{CardDef, Supertypes};
use beryl_scenario::Scenario;
use serde_json::Value;

use crate::{CARDS, props};

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
    let json = serde_json::to_value(beryl_scenario::Json(scenario)).expect("a scenario serializes");
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
    for prop in props::ALL {
        names_of(card(prop.card), &mut allowed);
    }
    allowed
}

#[test]
fn scenarios_name_only_their_card_its_tokens_basic_lands_and_props() {
    let known = every_name();
    let shared = shared();
    let mut failures = Vec::new();
    let generated = crate::derive::all().iter().copied();
    for (name, scenarios) in crate::scenarios()
        .iter()
        .copied()
        .chain(crate::library::by_card())
        .chain(generated)
    {
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

/// Each prop's card has exactly what its contract says, so a scenario can rely on the contract
/// and a reader never needs to know which card fills the role. A contract that says `""` for
/// its text is held to a card with no rules text beyond its keywords.
#[test]
fn every_prop_card_keeps_its_contract() {
    use beryl_card::SubtypeWords;

    use crate::props::Stats;

    let mut failures = Vec::new();
    for prop in props::ALL {
        let face = card(prop.card).front();
        let stats = match (face.power, face.toughness, face.loyalty) {
            (Some(power), Some(toughness), _) => Stats::Creature { power, toughness },
            (_, _, Some(loyalty)) => Stats::Planeswalker { loyalty },
            _ => Stats::None,
        };
        let textless = face.abilities.is_empty()
            && face.casting.is_empty()
            && face.additional_cost.is_empty()
            && face.grants.is_empty();
        let checks = [
            ("cost", face.cost == prop.cost),
            ("types", face.types == prop.types),
            (
                "subtypes",
                face.subtypes == SubtypeWords::new(prop.subtypes),
            ),
            ("stats", stats == prop.stats),
            ("keywords", face.keywords == prop.keywords),
            ("text", textless == prop.text.is_empty()),
        ];
        for (what, kept) in checks {
            if !kept {
                failures.push(format!(
                    "{}: its {what} differs from its contract: {face:?}",
                    prop.card
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
