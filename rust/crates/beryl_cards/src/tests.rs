//! The generated names, the card table and the decklists.

use std::fmt::Write;
use std::path::Path;

use beryl_engine::card::{AbilityKind, ManaColor, Subtype, Supertypes, Types};
use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Outcome, Profile, Replay, Scratch, State, Step,
    apply, next,
};

use crate::{CARDS, SCENARIOS, decks, names};

const CSV: &str = "data/vintage-cube-2026-10-07.csv";
const BASIC_LANDS: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];

/// A card name as a Rust constant: "Jace, Vryn's Prodigy" is `JACE_VRYNS_PRODIGY`.
fn constant(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            c => c,
        };
        assert!(c.is_ascii(), "no ASCII spelling for {c:?} in {name:?}");
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
        } else if c != '\'' && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    assert!(
        out.starts_with(|c: char| c.is_ascii_alphabetic()),
        "{name:?} gives no constant"
    );
    out
}

/// The names in the cube list, in its order.
fn cube_names(csv: &str) -> Vec<String> {
    csv.lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (_color, name) = line.split_once(',').expect("a color,name row");
            match name.strip_prefix('"') {
                Some(quoted) => quoted.strip_suffix('"').unwrap().replace("\"\"", "\""),
                None => name.to_string(),
            }
        })
        .collect()
}

fn generate(csv: &str) -> String {
    let mut out = String::from(
        "//! Every card name as a constant, so a misspelled card is a compile error.\n\
         //!\n\
         //! Generated from `data/vintage-cube-2026-10-07.csv` and the basic lands by the\n\
         //! `names_are_current` test. Regenerate with\n\
         //! `BERYL_UPDATE_NAMES=1 cargo test -p beryl_cards names_are_current`.\n\n",
    );
    let mut seen = std::collections::BTreeSet::new();
    let mut emit = |out: &mut String, name: &str| {
        let c = constant(name);
        assert!(seen.insert(c.clone()), "two names give {c}");
        writeln!(out, "pub const {c}: &str = {name:?};").unwrap();
    };
    for name in cube_names(csv) {
        emit(&mut out, &name);
    }
    out.push_str("\n// The basic lands, which decks add at deckbuilding.\n");
    for name in BASIC_LANDS {
        emit(&mut out, name);
    }
    out
}

#[test]
fn names_are_current() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let csv = std::fs::read_to_string(crate_dir.join("../../..").join(CSV)).unwrap();
    let expected = generate(&csv);
    let path = crate_dir.join("src/names.rs");
    if std::env::var_os("BERYL_UPDATE_NAMES").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "src/names.rs is stale: run `BERYL_UPDATE_NAMES=1 cargo test -p beryl_cards names_are_current`"
    );
}

#[test]
fn constants_spell_names_plainly() {
    assert_eq!(constant("Jace, Vryn's Prodigy"), "JACE_VRYNS_PRODIGY");
    assert_eq!(constant("Life // Death"), "LIFE_DEATH");
    assert_eq!(constant("Troll of Khazad-dûm"), "TROLL_OF_KHAZAD_DUM");
    assert_eq!(names::PALANTIR_OF_ORTHANC, "Palantír of Orthanc");
}

#[test]
fn the_wu_control_deck_is_forty_cards_of_twenty_nine_names() {
    assert_eq!(decks::WU_CONTROL.len(), 29);
    let total: u32 = decks::WU_CONTROL.iter().map(|&(n, _)| u32::from(n)).sum();
    assert_eq!(total, 40);
}

#[test]
fn every_card_file_is_in_the_table_under_its_own_name() {
    let mut names: Vec<_> = CARDS.cards().iter().map(|c| c.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), CARDS.cards().len());
    for card in CARDS.cards() {
        assert!(std::ptr::eq(CARDS.get(card.name).unwrap(), *card));
    }
}

#[test]
fn tundra_is_a_plains_island_with_both_mana_abilities() {
    let tundra = CARDS.get(names::TUNDRA).unwrap().front();
    assert_eq!(tundra.types, Types::LAND);
    assert!(!tundra.supertypes.contains(Supertypes::BASIC));
    assert!(tundra.subtypes.contains(Subtype::Plains));
    assert!(tundra.subtypes.contains(Subtype::Island));
    let produced: Vec<_> = tundra
        .abilities
        .iter()
        .map(|a| {
            assert_eq!(a.kind, AbilityKind::Mana);
            assert!(a.cost.tap);
            (
                a.produces.includes(ManaColor::White),
                a.produces.includes(ManaColor::Blue),
            )
        })
        .collect();
    assert_eq!(produced, [(true, false), (false, true)]);
    for (name, color) in [
        (names::PLAINS, ManaColor::White),
        (names::ISLAND, ManaColor::Blue),
    ] {
        let basic = CARDS.get(name).unwrap().front();
        assert!(basic.supertypes.contains(Supertypes::BASIC));
        assert_eq!(basic.abilities.len(), 1);
        assert!(basic.abilities[0].produces.includes(color));
    }
}

#[test]
fn a_game_of_the_three_lands_runs_to_decking() {
    let deck = &[
        (14, names::PLAINS),
        (13, names::ISLAND),
        (13, names::TUNDRA),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 42, Profile::SMART);
    let mut x = Scratch::new();
    let choose = |d: &Decision| match d.kind {
        DecisionKind::Priority => Choice((d.len() - 1) as u8),
        _ => Choice(0),
    };
    let outcome = loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => apply(&mut s, &t, &mut x, &mut Replay, choose(&d)),
        }
    };
    assert_eq!(outcome, Outcome::Win(s.turn().starting_player));
    assert_eq!(s.battlefield().len(), 67);
}

#[test]
fn every_card_has_scenarios_and_they_pass() {
    let mut cards: Vec<_> = SCENARIOS.iter().map(|&(name, _)| name).collect();
    cards.sort_unstable();
    let mut table: Vec<_> = CARDS.cards().iter().map(|c| c.name).collect();
    table.sort_unstable();
    assert_eq!(cards, table, "one SCENARIOS entry per card");
    let mut failures = Vec::new();
    for &(card, scenarios) in SCENARIOS {
        assert!(!scenarios.is_empty(), "{card} has no scenario");
        for scenario in scenarios {
            if let Err(e) = beryl_scenario::run(&CARDS, scenario) {
                failures.push(format!("{card}: {e}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_dump_is_valid_json_with_every_scenario() {
    let json: serde_json::Value =
        serde_json::from_str(&beryl_scenario::to_json(SCENARIOS)).unwrap();
    let total: usize = SCENARIOS.iter().map(|(_, s)| s.len()).sum();
    assert_eq!(json.as_array().unwrap().len(), total);
    assert_eq!(json[0]["card"], names::PLAINS);
}
