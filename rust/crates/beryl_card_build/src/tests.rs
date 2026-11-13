use std::path::{Path, PathBuf};

use crate::files::{card_files, set_crates};
use crate::names::{card_module, constant, set_module};
use crate::printed::Subtypes;
use crate::store::Store;
use crate::{Printed, registry_source, set_crate_source, shared_source};

/// A record's line: the fields the reader needs, a card's first set and its faces' JSON.
fn record(kind: &str, name: &str, id: &str, first_set: Option<&str>, faces: &[&str]) -> String {
    let first = first_set.map_or(String::new(), |set| format!(",\"first_set\":\"{set}\""));
    let escaped = name.replace('"', "\\\"");
    format!(
        "{kind}:{name}\t{{\"name\":\"{escaped}\",\"oracle_id\":\"o\",\"scryfall_id\":\"{id}\",\
         \"set\":\"s\",\"collector_number\":\"1\"{first},\"layout\":\"{}\",\"faces\":[{}]}}\n",
        if kind == "token" { "token" } else { "normal" },
        faces.join(","),
    )
}

/// A face's JSON: its name, type line and the other fields given as JSON.
fn face(name: &str, type_line: &str, rest: &str) -> String {
    format!("{{\"name\":\"{name}\",\"type_line\":\"{type_line}\",\"side\":\"front\"{rest}}}")
}

/// The test store, its lines sorted as `just cards` writes them.
fn store_text() -> String {
    let human = |id, color| {
        let rest = format!(",\"colors\":[\"{color}\"],\"power\":\"1\",\"toughness\":\"1\"");
        record(
            "token",
            "Human Token",
            id,
            None,
            &[&face("Human Token", "Token Creature — Human", &rest)],
        )
    };
    let adeline = face(
        "Adeline",
        "Legendary Creature — Human Knight",
        ",\"mana_cost\":\"{1}{W}{W}\",\"colors\":[\"W\"],\"power\":\"*\",\"toughness\":\"4\"",
    );
    let mut lines = [
        record("card", "Adeline", "a", Some("mid"), &[&adeline]).replace(
            "\"faces\"",
            "\"tokens\":[{\"name\":\"Human Token\",\"scryfall_id\":\"h2\"}],\"faces\"",
        ),
        record(
            "card",
            "Fire // Ice",
            "x",
            Some("apc"),
            &[
                &face(
                    "Fire",
                    "Instant",
                    ",\"mana_cost\":\"{1}{R}\",\"colors\":[\"R\"]",
                ),
                &face(
                    "Ice",
                    "Instant",
                    ",\"mana_cost\":\"{1}{U}\",\"colors\":[\"U\"]",
                ),
            ],
        ),
        "card:Henzie\t{\"alias\":\"Henzie \\\"Toolbox\\\" Torre\",\"scryfall_id\":\"y\"}\n".into(),
        record("card", "Henzie \"Toolbox\" Torre", "y", Some("snc"), &[]),
        "card:Ice\t{\"alias\":\"Fire // Ice\",\"scryfall_id\":\"x\"}\n".into(),
        record(
            "card",
            "Island",
            "i",
            Some("lea"),
            &[&face("Island", "Basic Land — Island", "")],
        ),
        record("card", "Lim-Dûl's Vault", "v", Some("all"), &[]),
        record(
            "card",
            "Plains",
            "p",
            Some("lea"),
            &[&face("Plains", "Basic Land — Plains", "")],
        ),
        human("h1", "W"),
        human("h2", "R"),
        record("token", "Island", "t", None, &[]),
    ];
    lines.sort();
    lines.concat()
}

/// The subtypes these tests' engine has.
fn subtypes() -> Subtypes {
    Subtypes::named(&["Plains", "Island", "Human", "Knight"])
}

/// `set_crate_source` of the crate at `dir` over the test store.
fn crate_source(dir: &Path, store: &Store) -> Result<String, String> {
    set_crate_source(dir, "../crate", &mut Printed::new(store, &subtypes()))
}

/// A fresh directory under the system's temporary one, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("beryl_card_build-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The same spellings `tools/tests/test_set_groups.py` holds Python's rules to.
#[test]
fn modules_and_constants_spell_names_plainly() {
    assert_eq!(set_module("m10"), "m10");
    assert_eq!(set_module("10e"), "set_10e");
    assert_eq!(set_module("mod"), "set_mod");
    assert_eq!(card_module("Lim-Dûl's Vault").unwrap(), "lim_duls_vault");
    assert_eq!(card_module("Fire // Ice").unwrap(), "fire_ice");
    assert_eq!(card_module("+2 Mace").unwrap(), "card_2_mace");
    assert_eq!(card_module("Static").unwrap(), "card_static");
    assert_eq!(
        constant("Jace, Vryn's Prodigy").unwrap(),
        "JACE_VRYNS_PRODIGY"
    );
    assert_eq!(constant("Life // Death").unwrap(), "LIFE_DEATH");
    assert_eq!(
        constant("Troll of Khazad-dûm").unwrap(),
        "TROLL_OF_KHAZAD_DUM"
    );
    assert!(card_module("Ætherize").is_err());
    assert!(card_module("!?").is_err());
}

#[test]
fn the_store_finds_a_card_by_its_file_or_its_constant() {
    let store = Store::parse(&store_text()).unwrap();
    assert_eq!(store.in_set("lea", "island"), Ok("Island"));
    assert_eq!(store.in_set("apc", "fire_ice"), Ok("Fire // Ice"));
    // A face answers through its card's record.
    assert_eq!(store.in_set("apc", "ice"), Ok("Ice"));
    assert_eq!(store.in_set("snc", "henzie"), Ok("Henzie"));
    assert_eq!(store.by_constant("LIM_DULS_VAULT"), Ok("Lim-Dûl's Vault"));
    assert!(
        store
            .in_set("lea", "iland")
            .unwrap_err()
            .contains("no card")
    );
    let misplaced = store.in_set("apc", "island").unwrap_err();
    assert!(misplaced.contains("first printed in lea"), "{misplaced}");
    assert!(store.by_constant("ILAND").is_err());
}

/// A second card in a set costs one new file: nothing else lists it.
#[test]
fn a_new_file_is_listed_with_no_other_edit() {
    let scratch = Scratch::new("new-file");
    let store = Store::parse(&store_text()).unwrap();
    scratch.write("cards_1993/Cargo.toml", "");
    scratch.write("cards_1993/lib.rs", "");
    scratch.write(
        "cards_1993/lea/plains.rs",
        "names::PLAINS names::ISLAND names::*",
    );
    let dir = scratch.0.join("cards_1993");
    let modules = |dir: &Path| -> Vec<String> {
        card_files(dir)
            .into_iter()
            .map(|f| format!("{}::{}", f.set, f.module))
            .collect()
    };
    assert_eq!(modules(&dir), ["lea::plains"]);
    let before = crate_source(&dir, &store).unwrap();
    assert!(
        before.contains("pub const ISLAND: &str = \"Island\";"),
        "{before}"
    );
    assert!(!before.contains("lea::island::CARD"), "{before}");

    scratch.write("cards_1993/lea/island.rs", "names::ISLAND");
    scratch.write("cards_1993/apc/fire_ice.rs", "names::FIRE_ICE");
    assert_eq!(
        modules(&dir),
        ["apc::fire_ice", "lea::island", "lea::plains"]
    );
    let after = crate_source(&dir, &store).unwrap();
    for line in [
        "#[path = \"../crate/apc\"]\npub mod apc {",
        "    pub mod island;",
        "    &apc::fire_ice::CARD,\n    &lea::island::CARD,\n    &lea::plains::CARD,",
        "    (names::ISLAND, lea::island::scenarios::SCENARIOS),",
        "    pub const FIRE_ICE: &str = \"Fire // Ice\";",
    ] {
        assert!(after.contains(line), "no {line:?} in:\n{after}");
    }

    scratch.write("decks/a.txt", "4 Lim-Dûl's Vault\n\n1 Island\n");
    scratch.write("not_a_crate/lea/island.rs", "");
    let crates: Vec<String> = set_crates(&scratch.0)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(crates, ["cards_1993"]);
    let (tables, names) = registry_source(&scratch.0, &scratch.0.join("decks"), &store).unwrap();
    assert!(tables.contains("    cards_1993::CARDS,\n"), "{tables}");
    assert!(tables.contains("    cards_1993::SCENARIOS,\n"), "{tables}");
    for constant in ["FIRE_ICE", "FOREST", "ISLAND", "LIM_DULS_VAULT", "PLAINS"] {
        assert!(
            names.contains(&format!("pub const {constant}: &str = ")),
            "{names}"
        );
    }
}

#[test]
fn a_misspelled_file_or_constant_fails_the_build() {
    let scratch = Scratch::new("misspelled");
    let store = Store::parse(&store_text()).unwrap();
    scratch.write("lea/iland.rs", "");
    assert!(
        crate_source(&scratch.0, &store)
            .unwrap_err()
            .contains("lea/iland.rs")
    );
    std::fs::remove_file(scratch.0.join("lea/iland.rs")).unwrap();
    scratch.write("lea/island.rs", "names::PLANES");
    assert!(
        crate_source(&scratch.0, &store)
            .unwrap_err()
            .contains("names::PLANES")
    );
}

/// Every name in the real store can be spelled, so a refresh that brings a new accented
/// letter fails here and not in a build script.
#[test]
fn every_card_in_the_store_has_a_module() {
    // Read at run time: a test binary in a copied target directory outlives its checkout.
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    Store::read(&crate_dir.join("../../../data/card-store.tsv")).unwrap();
}

/// Each face's printed characteristics come from its record: a later face names itself, `*`
/// is 0, and a token is the printing its card links.
#[test]
fn faces_and_tokens_start_from_their_records() {
    let scratch = Scratch::new("printed");
    let store = Store::parse(&store_text()).unwrap();
    scratch.write(
        "mid/adeline.rs",
        "printed::ADELINE printed::token::HUMAN_TOKEN",
    );
    scratch.write("apc/fire_ice.rs", "printed::FIRE printed::ICE");
    let source = crate_source(&scratch.0, &store).unwrap();
    for line in [
        "pub const ADELINE: beryl_card::Face = beryl_card::Face::EMPTY\n    \
         .cost(beryl_card::mana(\"{1}{W}{W}\"))\n    \
         .supertypes(beryl_card::Supertypes::LEGENDARY)\n    \
         .types(beryl_card::Types::CREATURE)\n    \
         .subtypes(beryl_card::Subtypes::of(&[beryl_card::Subtype::Human, \
         beryl_card::Subtype::Knight]))\n    \
         .colors(beryl_card::Colors::WHITE)\n    \
         .power_toughness(0, 4);",
        "pub const FIRE: beryl_card::Face = beryl_card::Face::EMPTY\n    .cost(",
        "pub const ICE: beryl_card::Face = beryl_card::Face::EMPTY\n    .named(\"Ice\")\n",
        "pub const HUMAN_TOKEN: beryl_card::Face = beryl_card::Face::EMPTY\n    \
         .types(beryl_card::Types::CREATURE)\n    \
         .subtypes(beryl_card::Subtypes::of(&[beryl_card::Subtype::Human]))\n    \
         .colors(beryl_card::Colors::RED)",
    ] {
        assert!(source.contains(line), "no {line:?} in:\n{source}");
    }

    // With no card of the crate to choose a printing, the store's must agree.
    let shared = Scratch::new("printed-shared");
    shared.write("tokens.rs", "printed::token::HUMAN_TOKEN");
    let subtypes = subtypes();
    let error = shared_source(&shared.0, &mut Printed::new(&store, &subtypes)).unwrap_err();
    assert!(error.contains("printings of Human Token differ"), "{error}");
}

/// A card file never types what its record prints, and a face the engine cannot hold fails
/// the build by name.
#[test]
fn typed_or_unknown_characteristics_fail_the_build() {
    let scratch = Scratch::new("typed");
    let store = Store::parse(&store_text()).unwrap();
    scratch.write("lea/island.rs", "printed::ISLAND.types(Types::LAND)");
    let error = crate_source(&scratch.0, &store).unwrap_err();
    let island = scratch.0.join("lea").join("island.rs");
    assert!(
        error.starts_with(&format!("{}: ", island.display())) && error.contains(".types("),
        "{error}"
    );
    std::fs::remove_file(island).unwrap();
    scratch.write("lea/plains.rs", "printed::PLAINS");
    let without_plains = Subtypes::named(&["Island"]);
    let error =
        set_crate_source(&scratch.0, "..", &mut Printed::new(&store, &without_plains)).unwrap_err();
    assert!(
        error.contains("Plains: no subtype Plains in data/catalogs.json"),
        "{error}"
    );
}
