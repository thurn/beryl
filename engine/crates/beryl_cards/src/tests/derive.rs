//! The generated scenarios of the cards built from library effects: they pass where the hand-written ones do, they are the same each time,
//! and they exercise every ability of their card.

use beryl_engine::card::CardDef;
use beryl_engine::coverage::{self, address};
use beryl_scenario::{Json, Scenario};

use super::plays_alike_in_every_slot_order;
use crate::{CARDS, derive};

/// The cards with a Rust body, which get no generated scenarios: none since the port to
/// effect data.
const CUSTOM: &[&str] = &[];

/// The abilities of generated cards that no generated scenario exercises, and why. An entry
/// a generated scenario exercises fails too, so the list stays current.
const UNREACHED: &[(&str, &str)] = &[
    (
        "Cavern of Souls: ability 2 (Mana)",
        "its mana pays only for a creature spell of the type the land chose as it entered, and a \
         generated position puts the land onto the battlefield having chosen none; the card's \
         hand-written scenarios play it, choose Human and cast a Human with that mana",
    ),
    (
        "Panglacial Wurm: ability 0 (Static)",
        "the generator does not start the card in a library and play a search; the card's \
         hand-written scenarios cast it during the search",
    ),
    (
        "Spara's Headquarters: ability 0 (Mana)",
        "no card of the pool asks for {G}, and automatic payment spends a unit that may be \
         several kinds as the lowest of them, {W}",
    ),
    (
        "Thundertrap Trainer: ability 0 (Triggered)",
        "offspring's trigger goes on the stack with the card's own enters trigger, an order a \
         script names only by the card, and the look that follows asks a question the oracles \
         may ask of either trigger",
    ),
    (
        "Thundertrap Trainer: casting option 0 (Offspring(ManaCost { generic: 4, symbols: [0, \
         0, 0, 0, 0, 0], x: 0 }))",
        "as offspring's trigger",
    ),
];

/// `check` of each card's generated scenarios, the cards shared among threads: the failures,
/// in the order of the cards.
fn each_card(
    check: impl Fn(&'static CardDef, &'static [Scenario]) -> Vec<String> + Sync,
) -> Vec<String> {
    let cards = derive::all();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let chunk = cards.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = cards
            .chunks(chunk)
            .map(|chunk| {
                let check = &check;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .flat_map(|&(name, scenarios)| check(card(name), scenarios))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a check panicked"))
            .collect()
    })
}

fn card(name: &str) -> &'static CardDef {
    CARDS
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not a card"))
}

#[test]
fn the_cards_with_a_rust_body_are_listed() {
    assert_eq!(derive::custom(), CUSTOM);
}

/// Every card but those with a Rust body has generated scenarios, and each passes in the
/// strict runner under each numbering of the slots, as a hand-written one must.
#[test]
fn generated_scenarios_pass_in_every_slot_order() {
    let generated: Vec<&str> = derive::all().iter().map(|&(name, _)| name).collect();
    let expected: Vec<&str> = CARDS
        .cards()
        .iter()
        .map(|def| def.name)
        .filter(|name| !CUSTOM.contains(name))
        .collect();
    assert_eq!(generated, expected);
    let failures = each_card(|def, scenarios| {
        let mut failures: Vec<String> = scenarios
            .iter()
            .filter_map(|s| plays_alike_in_every_slot_order(def.name, s))
            .collect();
        if scenarios.is_empty() {
            failures.push(format!("{} has no generated scenario", def.name));
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Generation reads the definition and nothing else: generated again, a card's scenarios
/// are the same.
#[test]
fn generating_twice_gives_the_same_scenarios() {
    let json = |scenarios: &[Scenario]| {
        let scenarios: Vec<Json<Scenario>> = scenarios.iter().map(Json).collect();
        serde_json::to_string(&scenarios).expect("serializes")
    };
    let failures = each_card(|def, scenarios| {
        let again = derive::for_card(def).scenarios;
        if json(&again) == json(scenarios) {
            Vec::new()
        } else {
            vec![format!(
                "{} generates other scenarios the second time",
                def.name
            )]
        }
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Start each permission with an ordinary game action. A successful cast must have used
/// it: Snapcaster's spell is exiled on resolution and the Spellbinder's opponent adds a
/// second copy of the creature to the battlefield.
#[test]
fn generated_openings_use_permissions_from_graveyard_and_exile() {
    use crate::{names, props};
    use beryl_engine::Zone;
    use beryl_scenario::{P0, P1};
    use beryl_scenario_types::{ME, THEM};

    let cases = [
        (
            names::SNAPCASTER_MAGE,
            P0,
            props::SORCERY.card,
            ME.has(Zone::Exile, props::SORCERY.card, 1),
        ),
        (
            names::ELITE_SPELLBINDER,
            P1,
            props::VANILLA.card,
            THEM.controls(props::VANILLA.card, 2),
        ),
    ];
    for (source, caster, spell, assertion) in cases {
        let generated = derive::for_card(card(source));
        let used = generated.scenarios.iter().any(|scenario| {
            if !scenario.actions.iter().any(|action| matches!(
                action, beryl_scenario::Action::Cast { player, card, .. } if *player == caster && *card == spell
            )) {
                return false;
            }
            let mut actions = scenario.actions.to_vec();
            actions.push(assertion);
            beryl_scenario::run(&CARDS, &scenario.actions(Vec::leak(actions))).is_ok()
        });
        assert!(
            used,
            "{source} never generated a successful cast with the permission"
        );
    }
}

/// The abilities and casting options of `def`'s faces, each named for a report, with the
/// address coverage records it by.
fn parts_of(def: &'static CardDef) -> Vec<(String, usize)> {
    let mut parts = Vec::new();
    for (f, face) in def.faces.iter().enumerate() {
        let name = if f == 0 { def.name } else { face.name };
        for (i, a) in face.abilities.iter().enumerate() {
            parts.push((format!("{name}: ability {i} ({:?})", a.kind()), address(a)));
        }
        for (i, option) in face.casting.iter().enumerate() {
            let what = format!("{name}: casting option {i} ({option:?})");
            parts.push((what, address(option)));
        }
    }
    parts
}

/// Each ability and casting option of a generated card is exercised by one of its own
/// generated scenarios (architecture §17.3), but those [`UNREACHED`] lists.
#[test]
fn every_ability_of_a_generated_card_is_exercised_by_its_generated_scenarios() {
    let failures = each_card(|def, scenarios| {
        coverage::start();
        for scenario in scenarios {
            // `generated_scenarios_pass_in_every_slot_order` reports failures.
            let _ = beryl_scenario::run(&CARDS, scenario);
        }
        let hits = coverage::take();
        let mut failures = Vec::new();
        for (what, address) in parts_of(def) {
            let listed = UNREACHED.iter().any(|&(u, _)| u == what);
            match (hits.contains(&address), listed) {
                (false, false) => failures.push(format!("no generated scenario exercises {what}")),
                (true, true) => {
                    failures.push(format!("a generated scenario exercises {what}: unlist it"));
                }
                _ => {}
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
