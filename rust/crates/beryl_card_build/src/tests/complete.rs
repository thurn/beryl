//! The cards complete from the card store (`complete.rs`): the rule, and the definitions a set
//! crate's build writes for them.

use std::path::PathBuf;

use super::{Scratch, face, record};
use crate::complete::{Group, Keywords, complete_from_store, group_sets};
use crate::files::card_files;
use crate::names::set_module;
use crate::printed::faces;
use crate::store::Store;
use crate::{Printed, set_crate_source};

/// The keywords of a `Keywords` bitset written as `beryl_card` writes it.
fn keywords() -> Keywords {
    Keywords::parse(
        "bitset! {\n    Keywords(u64) {\n        /// Doc.\n        FIRST_STRIKE = 0,\n        \
         FLYING = 8,\n        VIGILANCE = 10,\n    }\n}\n",
    )
    .unwrap()
}

/// A single-faced card of `type_line`, `cost` and Oracle `text`.
fn card(name: &str, type_line: &str, cost: &str, text: &str) -> beryl_card_store::Record {
    let rest = format!(
        ",\"mana_cost\":\"{cost}\",\"oracle_text\":\"{text}\",\"power\":\"2\",\"toughness\":\"2\""
    );
    let line = record(
        "card",
        name,
        "c",
        Some("lea"),
        &[&face(name, type_line, &rest)],
    );
    let (_, value) = line.trim_end().split_once('\t').unwrap();
    serde_json::from_str(value).unwrap()
}

#[test]
fn the_rule_reads_the_engines_keywords_from_its_source() {
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source =
        std::fs::read_to_string(crate_dir.join("../beryl_card/src/characteristics.rs")).unwrap();
    let engine = Keywords::parse(&source).unwrap();
    let flyer = card("Wind Drake", "Creature — Drake", "{2}{U}", "Flying");
    assert_eq!(complete_from_store(&flyer, &engine), Some(vec!["FLYING"]));
    let striker = card("Fencer", "Creature — Human", "{W}", "First strike");
    assert_eq!(
        complete_from_store(&striker, &engine),
        Some(vec!["FIRST_STRIKE"])
    );
}

#[test]
fn a_creature_with_no_text_or_only_keywords_is_complete() {
    let k = keywords();
    let bear = card("Bear", "Creature — Bear", "{1}{G}", "");
    assert_eq!(complete_from_store(&bear, &k), Some(vec![]));
    let angel = card(
        "Angel",
        "Creature — Angel",
        "{3}{W}{W}",
        "Flying, vigilance",
    );
    assert_eq!(
        complete_from_store(&angel, &k),
        Some(vec!["FLYING", "VIGILANCE"])
    );
    // Reminder text is no ability (CR 207.2a); a keyword per line reads as a list.
    let reminded = card(
        "Drake",
        "Artifact Creature — Drake",
        "{4}",
        "Flying (It can't be blocked except by creatures with flying or reach.)\\nVigilance",
    );
    assert_eq!(
        complete_from_store(&reminded, &k),
        Some(vec!["FLYING", "VIGILANCE"])
    );
}

#[test]
fn anything_more_needs_a_file() {
    let k = keywords();
    let needs_file = [
        // A keyword the engine lacks.
        card("Trampler", "Creature — Beast", "{G}", "Trample"),
        // An ability that is not a keyword.
        card(
            "Pinger",
            "Creature — Wizard",
            "{R}",
            "Flying\\nWhen this enters, draw a card.",
        ),
        // A cost the engine cannot hold.
        card("Hybrid", "Creature — Elf", "{1}{G/W}", ""),
        // No creature.
        card("Wall Art", "Artifact", "{1}", ""),
    ];
    for record in &needs_file {
        assert_eq!(complete_from_store(record, &k), None, "{}", record.name);
    }
    let mut token = card("Bear Token", "Token Creature — Bear", "", "");
    token.first_set = None;
    assert_eq!(complete_from_store(&token, &k), None);
}

/// A set crate writes a definition for each complete card of its sets that has no file, and
/// leaves one with a file to the file.
#[test]
fn a_set_crate_defines_its_complete_cards_without_files() {
    let rest = ",\"mana_cost\":\"{1}{G}\",\"colors\":[\"G\"],\"power\":\"2\",\"toughness\":\"2\"";
    let flyer = ",\"mana_cost\":\"{2}{U}\",\"oracle_text\":\"Flying\",\"colors\":[\"U\"],\
                 \"power\":\"2\",\"toughness\":\"2\"";
    let mut lines = [
        record(
            "card",
            "Bear",
            "b",
            Some("lea"),
            &[&face("Bear", "Creature — Bear", rest)],
        ),
        record(
            "card",
            "Drake",
            "d",
            Some("lea"),
            &[&face("Drake", "Creature — Drake", flyer)],
        ),
        record(
            "card",
            "Elk",
            "e",
            Some("lea"),
            &[&face("Elk", "Creature — Elk", rest)],
        ),
        record(
            "card",
            "Other",
            "o",
            Some("mir"),
            &[&face("Other", "Creature — Bear", rest)],
        ),
    ];
    lines.sort();
    let store = Store::parse(&lines.concat()).unwrap();
    let scratch = Scratch::new("complete");
    scratch.write(
        "lea/elk.rs",
        "use crate::{names, printed};\npub static CARD: beryl_card::CardDef = \
         beryl_card::card(names::ELK, &[printed::ELK]);\n",
    );
    let sets = vec!["lea".to_string()];
    let k = keywords();
    let group = Group {
        sets: &sets,
        keywords: &k,
    };
    let named = crate::printed::Subtypes::named(&["Bear", "Drake", "Elk"]);
    let source =
        set_crate_source(&scratch.0, "..", &group, &mut Printed::new(&store, &named)).unwrap();
    assert!(source.contains(
        "pub static BEAR: beryl_card::CardDef = beryl_card::card(names::BEAR, &[printed::BEAR]);"
    ));
    assert!(source.contains(
        "beryl_card::card(names::DRAKE, &[printed::DRAKE.keywords(beryl_card::Keywords::FLYING)])"
    ));
    assert!(source.contains("    &store::BEAR,\n    &store::DRAKE,\n];"));
    assert!(!source.contains("store::ELK"), "the file defines Elk");
    assert!(!source.contains("OTHER"), "Other is of another group");
}

/// Every group with a card complete from the real store has a set crate, whose build defines
/// it. `just card crates` writes the crates by Python's copy of the rule (`store_cards.py`), so
/// were the two rules to drift, a card this rule completes could have no crate and be no card
/// at all. Also prints how many cards are complete, which the plan reports.
#[test]
fn every_card_complete_from_the_store_has_a_crate() {
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = crate_dir.join("../../..");
    let store = Store::read(&root.join("data/card-store.tsv")).unwrap();
    let groups = std::fs::read_to_string(root.join("data/set-groups.json")).unwrap();
    let engine = Keywords::parse(
        &std::fs::read_to_string(root.join(crate::complete::KEYWORDS_SOURCE)).unwrap(),
    )
    .unwrap();
    let catalogs = crate::catalogs::Catalogs::read(&root).unwrap();
    let subtypes = crate::printed::Subtypes::of_catalogs(&catalogs);
    let parsed: serde_json::Value = serde_json::from_str(&groups).unwrap();
    let (mut complete, mut with_files) = (0, 0);
    for group in parsed["groups"].as_array().unwrap() {
        let name = group["crate"].as_str().unwrap();
        let sets = group_sets(&groups, name).unwrap();
        let files: Vec<String> = card_files(&root.join("rust/cards").join(name))
            .into_iter()
            .map(|f| store.in_set(&f.set, &f.module).unwrap().to_string())
            .collect();
        for card in store.first_printed_in(&sets) {
            let record = store.card(card).unwrap();
            if complete_from_store(&record, &engine).is_some() && faces(&record, &subtypes).is_ok()
            {
                complete += 1;
                with_files += usize::from(files.iter().any(|f| f == card));
                assert!(
                    root.join("rust/cards")
                        .join(name)
                        .join("Cargo.toml")
                        .is_file(),
                    "{card} is complete from the store, but its group {name} has no set crate: \
                     run `just card crates`, and if that writes none, make store_cards.py's rule \
                     agree with beryl_card_build's complete.rs"
                );
            }
        }
        assert!(sets.iter().all(|s| !set_module(s).is_empty()));
    }
    println!("cards complete from the store: {complete}, of which {with_files} have files");
    assert!(complete > 0);
}
