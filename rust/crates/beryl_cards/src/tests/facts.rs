//! What the scenario dump says of each card ([`beryl_scenario::facts`]), read from its
//! definition: the facts the oracle exporters translate a scenario's decisions with, on cards
//! chosen so that each fact is met at least once.

use beryl_scenario::facts::{card_facts, named};
use serde_json::{Value, json};

use crate::{CARDS, names};

/// The facts of the card called `name`, as the dump writes them.
fn facts(name: &str) -> Value {
    serde_json::to_value(card_facts(CARDS.get(name).expect("a card of the table")))
        .expect("facts serialize")
}

/// The ability of `name`'s face `face` at `index` ("Own 1"), or of those the face grants.
fn ability(name: &str, face: usize, index: &str) -> Value {
    let face = &facts(name)["faces"][face];
    let list = if index.starts_with("Grant") {
        "grants"
    } else {
        "abilities"
    };
    face[list]
        .as_array()
        .expect("abilities")
        .iter()
        .find(|a| a["index"] == index)
        .unwrap_or_else(|| panic!("{index} of {face}"))
        .clone()
}

#[test]
fn an_activated_ability_says_its_cost_parts_and_targets() {
    assert_eq!(
        ability(names::STRIP_MINE, 0, "Own 1"),
        json!({
            "index": "Own 1",
            "kind": "Activated",
            "cost": {"parts": [
                {"action": "Tap", "of": "This"},
                {"action": "Sacrifice", "of": "This"},
            ]},
            "targets": [{"kind": "Permanent", "count": 1}],
            "instructions": [{"kind": "Destroy", "objects": "Target 0"}],
        })
    );
}

#[test]
fn a_mana_ability_says_the_mana_it_makes_and_whether_it_is_conditional() {
    let strip = ability(names::STRIP_MINE, 0, "Own 0");
    assert_eq!(strip["kind"], "Mana");
    assert_eq!(
        strip["produces"],
        json!({"colors": ["Colorless"], "amount": 1})
    );
    let verge = facts(names::FLOODFARM_VERGE);
    let abilities = verge["faces"][0]["abilities"].as_array().unwrap();
    let made: Vec<_> = abilities
        .iter()
        .map(|a| &a["produces"]["colors"][0])
        .collect();
    assert_eq!(made, [&json!("White"), &json!("Blue")]);
    assert_eq!(abilities[0].get("conditional"), None);
    assert_eq!(abilities[1]["conditional"], true);
    let crypt = ability(names::MANA_CRYPT, 0, "Own 1");
    assert_eq!(crypt["produces"]["amount"], 2);
    // A source of two kinds of mana: one ability each.
    let tundra = facts(names::TUNDRA);
    let kinds: Vec<_> = tundra["faces"][0]["abilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| &a["produces"]["colors"][0])
        .collect();
    assert_eq!(kinds, [&json!("White"), &json!("Blue")]);
}

#[test]
fn a_loyalty_ability_says_its_loyalty_change() {
    let changes: Vec<Value> = (0..4)
        .map(|i| {
            let a = ability(names::JACE_THE_MIND_SCULPTOR, 0, &format!("Own {i}"));
            assert_eq!(a["kind"], "Loyalty");
            a["cost"]["parts"][0]["amount"].clone()
        })
        .collect();
    assert_eq!(changes, [json!(2), json!(0), json!(-1), json!(-12)]);
    assert_eq!(
        ability(names::JACE_THE_MIND_SCULPTOR, 0, "Own 1")["instructions"],
        json!([
            {"kind": "Draw", "player": "You", "count": 3},
            {"kind": "PutBack", "player": "You", "count": 2},
        ])
    );
    assert_eq!(
        ability(names::JACE_THE_MIND_SCULPTOR, 0, "Own 0")["instructions"][0],
        json!({"kind": "Fateseal", "player": "You", "others": "Target 0", "count": 1})
    );
}

#[test]
fn keyword_costs_say_what_pays_them() {
    // Waterbend: generic mana, each payable by tapping a permanent.
    let iceberg = ability(names::AANGS_ICEBERG, 0, "Own 1");
    assert_eq!(
        iceberg["cost"],
        json!({"mana": "{3}", "parts": [
            {"action": "Tap", "of": "Chosen", "zone": "Battlefield", "for_generic": 3},
        ]})
    );
    // Crew, granted by another card.
    let crew = ability(names::SWIFT_RECONFIGURATION, 0, "Grant 0");
    assert_eq!(crew["cost"]["parts"][0]["total_power"], 5);
    assert_eq!(crew["cost"]["parts"][0]["other"], true);
    // Cycling: discard this card, then draw.
    let cycling = ability(names::SPARAS_HEADQUARTERS, 0, "Own 4");
    assert_eq!(
        cycling["cost"],
        json!({"mana": "{3}", "parts": [{"action": "Discard", "of": "This"}]})
    );
    assert_eq!(cycling["instructions"][0]["kind"], "Draw");
    // A counter removed from this.
    let staff = ability(names::STAFF_OF_THE_STORYTELLER, 0, "Own 2");
    assert_eq!(
        staff["cost"],
        json!({"mana": "{W}", "parts": [
            {"action": "Tap", "of": "This"},
            {"action": "RemoveCounter", "of": "This", "counter": "Story"},
        ]})
    );
}

#[test]
fn casting_options_say_their_bit_keyword_and_cost() {
    let casting = |name: &str| facts(name)["faces"][0]["casting"].clone();
    assert_eq!(
        casting(names::SUBTLETY),
        json!([{"bit": 0, "keyword": "Evoke", "alternative": true, "cost": {"parts": [
            {"action": "Exile", "of": "Chosen", "zone": "Hand", "count": 1},
        ]}}])
    );
    assert_eq!(
        casting(names::WINDS_OF_ABANDON),
        json!([{"bit": 0, "keyword": "Overload", "alternative": true,
            "cost": {"mana": "{4}{W}{W}"}, "ability": "Own 1"}])
    );
    assert_eq!(
        casting(names::THUNDERTRAP_TRAINER),
        json!([{"bit": 0, "keyword": "Offspring", "alternative": false,
            "cost": {"mana": "{4}"}}])
    );
    assert_eq!(casting(names::THIEVING_SKYDIVER)[0]["cost"]["mana"], "{X}");
    assert_eq!(
        casting(names::OVERLORD_OF_THE_MISTMOORS)[0]["keyword"],
        "Impending"
    );
}

#[test]
fn triggered_abilities_say_their_event_in_ability_order() {
    let subtlety = facts(names::SUBTLETY);
    let abilities = subtlety["faces"][0]["abilities"].as_array().unwrap();
    let triggers: Vec<_> = abilities
        .iter()
        .map(|a| (&a["kind"], &a["trigger"], &a["instructions"][0]["kind"]))
        .collect();
    assert_eq!(
        triggers,
        [
            (
                &json!("Triggered"),
                &json!("Enters"),
                &json!("PutIntoLibrary")
            ),
            (&json!("Triggered"), &json!("Enters"), &json!("Sacrifice")),
        ]
    );
}

#[test]
fn library_instructions_say_who_how_many_and_whether_may() {
    // Scry.
    assert_eq!(
        ability(names::AANGS_ICEBERG, 0, "Own 1")["instructions"],
        json!([
            {"kind": "Sacrifice", "objects": "This"},
            {"kind": "Scry", "player": "You", "count": 2},
        ])
    );
    // "You may search": the search says may.
    assert_eq!(
        ability(names::TRINKET_MAGE, 0, "Own 0")["instructions"],
        json!([{"kind": "Search", "may": true, "player": "You", "count": 1,
            "rest": "RevealToHand"}])
    );
    // Each player searches: the players of the "each".
    assert_eq!(
        ability(names::WINDS_OF_ABANDON, 0, "Own 0")["instructions"][1],
        json!({"kind": "Search", "player": "EachPlayer", "count": "ThoseOf",
            "rest": "OntoBattlefieldTapped"})
    );
    // Look at the top five, take two.
    assert_eq!(
        ability(names::STOCK_UP, 0, "Own 0")["instructions"],
        json!([{"kind": "Dig", "player": "You", "count": 5, "take": 2, "rest": "Any"}])
    );
    // Look at the top four, you may reveal one.
    assert_eq!(
        ability(names::THUNDERTRAP_TRAINER, 0, "Own 1")["instructions"],
        json!([{"kind": "Dig", "player": "You", "count": 4, "take": "MayReveal",
            "rest": "Random"}])
    );
}

#[test]
fn choices_say_whether_they_may_choose_none() {
    let spellbind = ability(names::ELITE_SPELLBINDER, 0, "Own 0");
    let choose = spellbind["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "Choose")
        .unwrap()
        .clone();
    assert_eq!(
        choose,
        json!({"kind": "Choose", "player": "You", "objects": "InZoneOf Hand",
            "optional": true})
    );
    assert_eq!(
        ability(names::TIME_SPIRAL, 0, "Own 0")["instructions"][3],
        json!({"kind": "UntapUpTo", "player": "You", "objects": "All Battlefield",
            "count": 6})
    );
}

#[test]
fn unless_pays_says_who_pays_what_then_what_it_does() {
    assert_eq!(
        ability(names::MANA_TITHE, 0, "Own 0")["instructions"],
        json!([
            {"kind": "Unless", "player": "ControllerOf Target 0", "cost": {"mana": "{1}"}},
            {"kind": "Counter", "objects": "Target 0"},
        ])
    );
}

#[test]
fn targets_say_what_they_accept_and_faces_whether_they_transform() {
    let sink = facts(names::SINK_INTO_STUPOR);
    assert_eq!(
        sink["faces"][0]["abilities"][0]["targets"],
        json!([{"kind": "SpellOrPermanent", "count": 1}])
    );
    // A modal double-faced card's back is cast or played, not transformed into.
    assert_eq!(sink["faces"][1]["name"], "Soporific Springs");
    assert_eq!(sink["faces"][1].get("transformed"), None);
    let jace = facts(names::JACE_VRYNS_PRODIGY);
    assert_eq!(jace["faces"][1]["transformed"], true);
}

#[test]
fn a_scenario_names_its_cards_and_the_tokens_they_make() {
    let (card, scenarios) = crate::scenarios()
        .iter()
        .find(|(card, _)| *card == names::STAFF_OF_THE_STORYTELLER)
        .unwrap();
    let named = named(&CARDS, card, &scenarios[0]);
    assert!(named.contains_key(names::STAFF_OF_THE_STORYTELLER));
    assert!(
        named.contains_key(cards_2023_h1::onc::staff_of_the_storyteller::SPIRIT_TOKEN),
        "{:?}",
        named.keys()
    );
}
