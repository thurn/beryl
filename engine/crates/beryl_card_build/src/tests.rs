use std::path::{Path, PathBuf};

mod complete;

use crate::complete::Group;
use crate::files::{card_files, set_crates};
use crate::names::{camel, card_module, constant, constant_of_camel, set_module};
use crate::printed::{Subtypes, face_source, shape_source};
use crate::store::Store;
use crate::{Printed, names_source, registry_source, set_crate_source, shared_source};

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

#[test]
fn printed_defense_and_meld_links_are_read_from_the_store() {
    let face: beryl_card_store::Face =
        serde_json::from_str(&face("Test Battle", "Battle", ",\"defense\":\"5\"")).unwrap();
    let source = face_source(&face, 0, &subtypes()).unwrap();
    assert!(source.contains(".defense(5)"));

    let record: beryl_card_store::Record = serde_json::from_str(
        r#"{
            "name":"Test Meld", "oracle_id":"o", "scryfall_id":"c", "set":"s",
            "collector_number":"1", "layout":"meld",
            "meld":{"with":"Test Partner","into":"Test Result"}, "faces":[]
        }"#,
    )
    .unwrap();
    assert_eq!(
        shape_source(&record).as_deref(),
        Some(
            r#"beryl_card::definition::Shape::Meld { with: "Test Partner", into: "Test Result" }"#
        ),
    );
}

#[test]
fn a_prototype_line_becomes_the_faces_casting_option() {
    let text = ",\"mana_cost\":\"{7}\",\"power\":\"4\",\"toughness\":\"3\",\"oracle_text\":\
                \"Prototype {1}{U}{U} \u{2014} 2/1 (You may cast this spell with different mana \
                cost, color, and size.)\\nFlying\"";
    let face: beryl_card_store::Face =
        serde_json::from_str(&face("Test Proxy", "Artifact Creature", text)).unwrap();
    let source = face_source(&face, 0, &subtypes()).unwrap();
    assert!(
        source.contains(
            ".casting(&[beryl_card::CastingOption::prototype(beryl_card::mana(\"{1}{U}{U}\"), 2, 1)])"
        ),
        "{source}"
    );
    assert!(source.contains(".power_toughness(4, 3)"), "{source}");
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
    let mut lines = [
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
        )
        .replace("\"layout\":\"normal\"", "\"layout\":\"split\""),
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
    set_crate_source(
        dir,
        "../crate",
        &Group::NONE,
        &mut Printed::new(store, &subtypes()),
    )
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
fn camel_case_constants_spell_names_and_back() {
    for (name, spelled, constant) in [
        (
            "Jace, Vryn's Prodigy",
            "JaceVrynsProdigy",
            "JACE_VRYNS_PRODIGY",
        ),
        ("Fire // Ice", "FireIce", "FIRE_ICE"),
        ("+2 Mace", "Card2Mace", "CARD_2_MACE"),
        (
            "Troll of Khazad-dûm",
            "TrollOfKhazadDum",
            "TROLL_OF_KHAZAD_DUM",
        ),
    ] {
        assert_eq!(camel(name).unwrap(), spelled);
        assert_eq!(constant_of_camel(spelled), constant);
    }
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

    scratch.write(
        "cards_1993/lea/island.rs",
        "use names::ISLAND;\npub mod scenarios {}",
    );
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
        "    (names::FIRE_ICE, &[]),",
        "    (names::ISLAND, lea::island::scenarios::SCENARIOS),",
        "    pub const FIRE_ICE: &str = \"Fire // Ice\";",
    ] {
        assert!(after.contains(line), "no {line:?} in:\n{after}");
    }

    scratch.write("not_a_crate/lea/island.rs", "");
    let crates: Vec<String> = set_crates(&scratch.0)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(crates, ["cards_1993"]);
    let tables = registry_source(&scratch.0);
    assert!(tables.contains("    &cards_1993::CARDS,\n"), "{tables}");
    assert!(tables.contains("    &cards_1993::SCENARIOS,\n"), "{tables}");
    let own = after.split("pub mod names").next().unwrap();
    assert!(own.contains("pub const FIRE_ICE: &str = "), "{after}");
}

/// `beryl_card_names` names every card of the store, whether or not it has a file, so that no card file is one of its inputs.
#[test]
fn the_names_crate_names_every_card_of_the_store() {
    let store = Store::parse(&store_text()).unwrap();
    let names = names_source(&store);
    for line in [
        "pub const FIRE_ICE: &str = \"Fire // Ice\";",
        "pub const ISLAND: &str = \"Island\";",
        "pub const LIM_DULS_VAULT: &str = \"Lim-D\u{fb}l's Vault\";",
        // An alias, as a double-faced card's front face is.
        "pub const ICE: &str = \"Ice\";",
    ] {
        assert!(names.contains(line), "no {line:?} in:\n{names}");
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
    Store::parse(&beryl_test_files::read_to_string("data/card-store.tsv").unwrap()).unwrap();
}

/// A shared token has no card to select one printing, so its printings must agree.
#[test]
fn a_shared_token_rejects_conflicting_printings() {
    let store = Store::parse(&store_text()).unwrap();
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
    let error = set_crate_source(
        &scratch.0,
        "..",
        &Group::NONE,
        &mut Printed::new(&store, &without_plains),
    )
    .unwrap_err();
    assert!(
        error.contains("Plains: no subtype Plains in data/catalogs.json"),
        "{error}"
    );
}

/// A face with changeling, a characteristic-defining ability, prints every creature type, with
/// its printed ones too; reminder text and other words don't make one.
#[test]
fn a_changeling_face_is_every_creature_type() {
    let face = |text: &str| -> beryl_card_store::Face {
        serde_json::from_str(&face(
            "Shapeshifter",
            "Creature — Human",
            &format!(",\"oracle_text\":{text:?}"),
        ))
        .unwrap()
    };
    let every = "\n    .subtypes(beryl_card::Subtypes::every(beryl_card::Types::CREATURE)\
                 .union(beryl_card::Subtypes::of(&[beryl_card::Subtype::Human])))";
    for text in [
        "Changeling (This card is every creature type.)",
        "Flying, changeling",
    ] {
        let source = face_source(&face(text), 0, &subtypes()).unwrap();
        assert!(source.contains(every), "{text}: {source}");
    }
    let source = face_source(
        &face("Creatures you control have changeling."),
        0,
        &subtypes(),
    )
    .unwrap();
    assert!(
        source.contains("\n    .subtypes(beryl_card::Subtypes::of(&[beryl_card::Subtype::Human]))"),
        "{source}"
    );
}
