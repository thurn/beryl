//! The generated names, the card table and the decklists.

use std::fmt::Write;
use std::path::Path;

use beryl_engine::card::{AbilityKind, ManaColor, Subtype, Supertypes, Types};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome, Profile, Replay, Scratch,
    Seat, State, Step, TurnStep, Zone, apply, next,
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

/// The UI draws a card, token or emblem from `data/cards.json` by the name the engine gives
/// it, a face's name (`ui/src/cards.ts`): each must be there.
#[test]
fn every_face_token_and_emblem_name_is_in_the_card_data() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/cards.json");
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut known = std::collections::BTreeSet::new();
    for entry in ["cards", "tokens"]
        .iter()
        .flat_map(|k| data[k].as_array().unwrap())
    {
        known.insert(entry["name"].as_str().unwrap());
        for face in entry["faces"].as_array().unwrap() {
            known.insert(face["name"].as_str().unwrap());
        }
    }
    let mut defs: Vec<_> = CARDS.cards().to_vec();
    let mut i = 0;
    while i < defs.len() {
        for &token in defs[i].tokens {
            if !defs.iter().any(|d| std::ptr::eq(*d, token)) {
                defs.push(token);
            }
        }
        i += 1;
    }
    let mut missing: Vec<&str> = defs
        .iter()
        .flat_map(|d| d.face_ixs().map(|f| d.face_name(f)))
        .filter(|name| !known.contains(name))
        .collect();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "not in data/cards.json (run `just cards`): {missing:?}"
    );
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
        if scenarios.is_empty() {
            failures.push(format!("{card} has no scenario"));
        }
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
        serde_json::from_str(&beryl_scenario::to_json(&CARDS, SCENARIOS)).unwrap();
    let total: usize = SCENARIOS.iter().map(|(_, s)| s.len()).sum();
    assert_eq!(json.as_array().unwrap().len(), total);
    assert_eq!(json[0]["card"], names::PLAINS);
}

/// A paused body's looks stay known (architecture §13): while Elite Spellbinder asks whether
/// to exile a card, its controller knows the hand it looked at, so a state determinized from
/// what they know replays to the same choice of cards.
#[test]
fn a_question_after_a_look_keeps_the_look() {
    let p0 = &[(1, names::ELITE_SPELLBINDER), (3, names::PLAINS)];
    let p1 = &[
        (1, names::THIEVING_SKYDIVER),
        (1, names::SPHINX_OF_FALSE_CONCLUSIONS),
        (1, names::ISLAND),
    ];
    let t = GameTable::new(&CARDS, [p0, p1]).unwrap();
    let mut s = State::new(&t, 1, Profile::EXACT);
    s.begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    for player in Seat::BOTH {
        for slot in s.library(player).to_vec() {
            let land = t.def(s.card(slot)).front().types.contains(Types::LAND);
            let zone = if player == Seat::P0 && land {
                Zone::Battlefield
            } else {
                Zone::Hand
            };
            s.place(slot, zone).unwrap();
        }
    }
    let mut x = Scratch::new();
    let question = loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Decide(d) if d.kind == DecisionKind::YesNo => break d,
            Step::Decide(d) => {
                let cast = (0..d.len() as u8)
                    .map(Choice)
                    .find(|&c| matches!(d.candidates.option(c), Some(Opt::Cast { .. })));
                apply(&mut s, &t, &mut x, &mut Replay, cast.unwrap_or(Choice(0)));
            }
            Step::Over(o) => panic!("the game ended: {o:?}"),
        }
    };
    assert_eq!(question.player, Seat::P0);
    for slot in s.zone(Seat::P1, Zone::Hand).iter() {
        assert!(
            s.knows(Seat::P0, slot),
            "player 0 forgot the card in slot {slot}"
        );
    }
    let choices = |s: &mut State| {
        let mut x = Scratch::new();
        apply(s, &t, &mut x, &mut Replay, Choice(0));
        match next(s, &t, &mut x, &mut Replay) {
            Step::Decide(d) => d,
            Step::Over(o) => panic!("the game ended: {o:?}"),
        }
    };
    let mut det = Observation::new(&s, Seat::P0).determinize(&t, &mut SplitMix64::new(9));
    let real = choices(&mut s);
    assert_eq!(real.kind, DecisionKind::ChooseObject);
    assert_eq!(choices(&mut det), real);
}
