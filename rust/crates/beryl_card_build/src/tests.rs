use std::path::{Path, PathBuf};

use crate::files::{card_files, set_crates};
use crate::names::{card_module, constant, set_module};
use crate::store::Store;
use crate::{registry_source, set_crate_source};

const STORE: &str = concat!(
    "card:Fire // Ice\t{\"name\":\"Fire // Ice\",\"first_set\":\"apc\",\"faces\":[]}\n",
    "card:Henzie\t{\"alias\":\"Henzie \\\"Toolbox\\\" Torre\",\"scryfall_id\":\"y\"}\n",
    "card:Henzie \"Toolbox\" Torre\t{\"name\":\"Henzie \\\"Toolbox\\\" Torre\",\"first_set\":\"snc\"}\n",
    "card:Ice\t{\"alias\":\"Fire // Ice\",\"scryfall_id\":\"x\"}\n",
    "card:Island\t{\"name\":\"Island\",\"first_set\":\"lea\"}\n",
    "card:Lim-Dûl's Vault\t{\"name\":\"Lim-Dûl's Vault\",\"first_set\":\"all\"}\n",
    "card:Plains\t{\"name\":\"Plains\",\"first_set\":\"lea\"}\n",
    "token:Island\t{\"name\":\"Island\"}\n",
);

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
    let store = Store::parse(STORE).unwrap();
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
    let store = Store::parse(STORE).unwrap();
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
    let before = set_crate_source(&dir, "../crate", &store).unwrap();
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
    let after = set_crate_source(&dir, "../crate", &store).unwrap();
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
    let store = Store::parse(STORE).unwrap();
    scratch.write("lea/iland.rs", "");
    assert!(
        set_crate_source(&scratch.0, "..", &store)
            .unwrap_err()
            .contains("lea/iland.rs")
    );
    std::fs::remove_file(scratch.0.join("lea/iland.rs")).unwrap();
    scratch.write("lea/island.rs", "names::PLANES");
    assert!(
        set_crate_source(&scratch.0, "..", &store)
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
