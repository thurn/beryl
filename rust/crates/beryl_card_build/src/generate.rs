//! The sources the build scripts write to `OUT_DIR`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::files::{card_files, set_crates};
use crate::names::constant;
use crate::printed::{Subtypes, faces};
use crate::store::{Record, Store};

/// The basic lands, which every `names` module of the registry has: the rules name them
/// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "The basic land types are
/// Plains, Island, Swamp, Mountain, and Forest").
const BASIC_LANDS: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];

/// The constants `text` reads from a module: each `<module>::<CONSTANT>`, where `module` is
/// `names::` or `printed::token::`.
fn constants_used<'a>(text: &'a str, module: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices(module) {
        let rest = &text[at + module.len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(rest.len());
        if end > 0 {
            found.push(&rest[..end]);
        }
    }
    found
}

/// The builder calls that would type a printed characteristic of a face in a card file, which
/// its `printed` constant holds instead (plan K2: "Nobody types a cost or type line").
const TYPED: [&str; 8] = [
    ".cost(",
    ".supertypes(",
    ".types(",
    ".subtypes(",
    ".colors(",
    ".power_toughness(",
    ".loyalty(",
    ".named(",
];

/// The first printed characteristic `text`, a card file or a shared module, types by hand.
pub fn typed_characteristic(text: &str) -> Option<&'static str> {
    TYPED.into_iter().find(|call| text.contains(call))
}

/// What a crate's card files and shared modules start their faces from: the faces of the
/// crate's own cards, by name, and the tokens they define.
pub struct Printed<'a> {
    pub store: &'a Store,
    pub subtypes: &'a Subtypes,
    /// The `(constant, source)` of each face, in order.
    faces: Vec<(String, String)>,
    /// The tokens the crate's cards make, as their records link them.
    links: Vec<beryl_card_store::TokenLink>,
    /// The `printed::token::` constants its files use.
    tokens: Vec<String>,
}

impl<'a> Printed<'a> {
    pub fn new(store: &'a Store, subtypes: &'a Subtypes) -> Self {
        Self {
            store,
            subtypes,
            faces: Vec::new(),
            links: Vec::new(),
            tokens: Vec::new(),
        }
    }

    /// Adds the faces of `record`, a card with a file in the crate, and the tokens it makes.
    fn card(&mut self, record: &Record) -> Result<(), String> {
        for (name, source) in faces(record, self.subtypes)? {
            let spelled = constant(&name)?;
            if self.faces.iter().any(|(c, _)| *c == spelled) {
                return Err(format!("two faces of this crate give printed::{spelled}"));
            }
            self.faces.push((spelled, source));
        }
        self.links.extend(record.tokens.iter().cloned());
        Ok(())
    }

    /// Reads a file of the crate: refuses a typed characteristic and notes the tokens it
    /// uses.
    fn file(&mut self, path: &Path, text: &str) -> Result<(), String> {
        if let Some(call) = typed_characteristic(text) {
            return Err(format!(
                "{}: a printed characteristic is typed by hand ({call}...): the face starts \
                 from its `printed::` constant, which the card store generates",
                path.display()
            ));
        }
        for used in constants_used(text, "printed::token::") {
            if !self.tokens.iter().any(|t| t == used) {
                self.tokens.push(used.to_string());
            }
        }
        Ok(())
    }

    /// The source of the token `printed::token::<spelled>`: the one record a card of the
    /// crate links, or the store's records of that name, which must agree.
    fn token(&self, spelled: &str) -> Result<String, String> {
        let (name, records) = self.store.tokens_by_constant(spelled)?;
        let linked: Vec<&Record> = records
            .iter()
            .filter(|r| {
                self.links
                    .iter()
                    .any(|l| l.name == name && l.scryfall_id == r.scryfall_id)
            })
            .collect();
        let chosen = if linked.is_empty() {
            records.iter().collect()
        } else {
            linked
        };
        let mut sources = Vec::new();
        for record in chosen {
            match &faces(record, self.subtypes)?[..] {
                [(_, source)] => sources.push(source.clone()),
                _ => return Err(format!("{name}: a token of more than one face")),
            }
        }
        sources.sort_unstable();
        sources.dedup();
        match &sources[..] {
            [source] => Ok(source.clone()),
            [] => Err(format!("{name}: no record in the card store")),
            _ => Err(format!(
                "printed::token::{spelled}: the store's printings of {name} differ; a card \
                 of this crate must make it to choose one"
            )),
        }
    }

    /// The `printed` module.
    fn source(&self) -> Result<String, String> {
        let mut out = String::from(
            "/// The printed characteristics of this crate's cards' faces and of the tokens its \
             files\n/// define, generated from the card store: where each face starts.\n\
             pub mod printed {\n",
        );
        for (spelled, source) in &self.faces {
            writeln!(out, "    pub const {spelled}: beryl_card::Face = {source};").unwrap();
        }
        out.push_str("    pub mod token {\n");
        for spelled in &self.tokens {
            let source = self.token(spelled)?;
            writeln!(
                out,
                "        pub const {spelled}: beryl_card::Face = {source};"
            )
            .unwrap();
        }
        out.push_str("    }\n}\n");
        Ok(out)
    }
}

/// A `names` module's constants, in order.
fn constants_source(constants: &BTreeMap<String, String>, indent: &str) -> String {
    let mut out = String::new();
    for (constant, name) in constants {
        writeln!(out, "{indent}pub const {constant}: &str = {name:?};").unwrap();
    }
    out
}

/// A set crate's source: the shared modules its files reach by `crate::`, the constants they
/// use, a module for each set directory with one for each file in it, and the crate's tables
/// in the files' order. `from_out` is the crate's directory as the written file reaches it, a
/// relative path so that a target directory copied to another checkout compiles that
/// checkout's files.
///
/// # Errors
///
/// When a file is no card of its set in the store, uses a constant that spells no card or
/// token, or types a printed characteristic, or a face's characteristics are none the engine
/// can hold.
pub fn set_crate_source(
    crate_dir: &Path,
    from_out: &str,
    printed: &mut Printed,
) -> Result<String, String> {
    let store = printed.store;
    let files = card_files(crate_dir);
    let mut constants = BTreeMap::new();
    let mut tree = String::new();
    let mut cards = String::new();
    let mut scenarios = String::new();
    let mut open: Option<&str> = None;
    for file in &files {
        let name = store.in_set(&file.set, &file.module)?;
        let own = constant(name)?;
        let text = std::fs::read_to_string(&file.path)
            .map_err(|e| format!("{}: {e}", file.path.display()))?;
        printed.card(&store.card(name)?)?;
        printed.file(&file.path, &text)?;
        for used in constants_used(&text, "names::") {
            if used != own && !constants.contains_key(used) {
                let named = store
                    .by_constant(used)
                    .map_err(|e| format!("{}: {e}", file.path.display()))?;
                constants.insert(used.to_string(), named.to_string());
            }
        }
        if open != Some(&file.set) {
            if open.is_some() {
                tree.push_str("}\n");
            }
            // An inline module's path is the directory its file modules are read from.
            writeln!(tree, "#[path = {:?}]", format!("{from_out}/{}", file.set)).unwrap();
            writeln!(tree, "pub mod {} {{", file.set).unwrap();
            open = Some(&file.set);
        }
        let (set, module) = (&file.set, &file.module);
        writeln!(tree, "    pub mod {module};").unwrap();
        writeln!(cards, "    &{set}::{module}::CARD,").unwrap();
        writeln!(
            scenarios,
            "    (names::{own}, {set}::{module}::scenarios::SCENARIOS),"
        )
        .unwrap();
        constants.insert(own, name.to_string());
    }
    if open.is_some() {
        tree.push_str("}\n");
    }
    Ok(format!(
        "// Generated by beryl_card_build from this crate's directories and the card store.\n\n\
         pub use beryl_card_shared::{{props, tokens}};\n\n\
         /// The names of this crate's cards and of the cards its files name.\n\
         pub mod names {{\n{}}}\n\n\
         {}\n\
         {tree}\n\
         /// This crate's cards, by set and file.\n\
         pub const CARDS: &[&beryl_card::CardDef] = &[\n{cards}];\n\n\
         /// Each card's scenarios, by its name, in the order of `CARDS`.\n\
         #[cfg(feature = \"scenarios\")]\n\
         pub const SCENARIOS: &[(&str, &[beryl_scenario_types::Scenario])] = &[\n{scenarios}];\n",
        constants_source(&constants, "    "),
        printed.source()?,
    ))
}

/// The shared crate's `printed` module: the tokens its modules in `dir` define.
///
/// # Errors
///
/// As [`Printed`]: a typed characteristic, or a token the store cannot settle.
pub fn shared_source(dir: &Path, printed: &mut Printed) -> Result<String, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .collect();
    paths.sort();
    for path in paths {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        printed.file(&path, &text)?;
    }
    printed.source()
}

/// The card names of the decklists in `dir`, main deck and sideboard: each `<count> <name>`
/// line of its `.txt` files.
fn deck_names(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let listing = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for path in listing.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for line in text.lines() {
            if let Some((count, name)) = line.split_once(' ')
                && count.parse::<u8>().is_ok()
            {
                names.push(name.trim().to_string());
            }
        }
    }
    Ok(names)
}

/// The registry's two sources: the tables' parts, one per set crate of `cards_dir` in the
/// crates' order, and the `names` constants: the cards of the decklists in `decks_dir`, the
/// basic lands and every card with a file.
///
/// # Errors
///
/// When a card file is no card of its set in the store, or two names give one constant.
pub fn registry_source(
    cards_dir: &Path,
    decks_dir: &Path,
    store: &Store,
) -> Result<(String, String), String> {
    let crates = set_crates(cards_dir);
    let mut names = deck_names(decks_dir)?;
    names.extend(BASIC_LANDS.map(str::to_string));
    for (_, dir) in &crates {
        for file in card_files(dir) {
            names.push(store.in_set(&file.set, &file.module)?.to_string());
        }
    }
    let mut constants = BTreeMap::new();
    for name in names {
        let spelled = constant(&name)?;
        if let Some(other) = constants.insert(spelled.clone(), name.clone())
            && other != name
        {
            return Err(format!("{other:?} and {name:?} both give names::{spelled}"));
        }
    }
    let list = |table: &str| -> String {
        let mut out = String::new();
        for (name, _) in &crates {
            writeln!(out, "    {name}::{table},").unwrap();
        }
        out
    };
    let tables = format!(
        "// Generated by beryl_card_build from the set crates under rust/cards.\n\n\
         /// Each set crate's cards, in the crates' order.\n\
         const SET_CARDS: &[&[&beryl_card::CardDef]] = &[\n{}];\n\n\
         /// Each set crate's scenarios by card name, in the crates' order.\n\
         #[cfg(feature = \"scenarios\")]\n\
         const SET_SCENARIOS: &[&[(&str, &[beryl_scenario_types::Scenario])]] = &[\n{}];\n",
        list("CARDS"),
        list("SCENARIOS"),
    );
    let names = format!(
        "// Generated by beryl_card_build from docs/sample-decks, the card files and the card \
         store.\n\n{}",
        constants_source(&constants, ""),
    );
    Ok((tables, names))
}

/// A path of the environment Cargo gives a build script.
fn env_path(variable: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} is not set")))
}

/// Writes `source` to `OUT_DIR/<file>`, or fails the build with the error.
fn write_out(file: &str, source: Result<String, String>) {
    let source = source.unwrap_or_else(|error| panic!("{error}"));
    std::fs::write(env_path("OUT_DIR").join(file), source).expect("OUT_DIR is writable");
}

/// `to` as a path from the directory `from`, with `/` between its parts; `to` itself where
/// they share no root, as on two drives. Both are resolved first: the compiler follows the
/// `..` of the result through the real directories, so a link on the way would mislead it.
fn relative(from: &Path, to: &Path) -> String {
    let resolved = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let (from, to) = (resolved(from), resolved(to));
    let (from, to): (Vec<_>, Vec<_>) = (from.components().collect(), to.components().collect());
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if shared == 0 {
        return to.iter().collect::<PathBuf>().display().to_string();
    }
    let up = std::iter::repeat_n("..".to_string(), from.len() - shared);
    let down = to[shared..]
        .iter()
        .map(|part| part.as_os_str().to_string_lossy().into_owned());
    up.chain(down).collect::<Vec<_>>().join("/")
}

/// The repository's root and its card store, from a crate two directories under `rust/`,
/// telling Cargo to run the script again when the store changes. Cargo keeps a relative path
/// by the crate's directory, so a target directory copied to another checkout watches that
/// checkout's files.
fn repository(manifest_dir: &Path) -> (PathBuf, Result<Store, String>) {
    let root = manifest_dir.join("../../..");
    println!("cargo:rerun-if-changed=../../../data/card-store.tsv");
    let store = Store::read(&root.join("data/card-store.tsv"));
    (root, store)
}

/// The subtypes the engine has, from `beryl_card`'s source under `root`, telling Cargo to run
/// the script again when they change.
fn subtypes(root: &Path) -> Result<Subtypes, String> {
    println!("cargo:rerun-if-changed=../../../rust/crates/beryl_card/src/characteristics.rs");
    let path = root.join("rust/crates/beryl_card/src/characteristics.rs");
    let source = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Subtypes::parse(&source)
}

/// A set crate's whole build script: writes `OUT_DIR/cards.rs`, which the crate's `lib.rs`
/// includes, and runs again when anything in the crate changes.
///
/// # Panics
///
/// With what is wrong, failing the build: [`set_crate_source`]'s errors.
pub fn set_crate() {
    let crate_dir = env_path("CARGO_MANIFEST_DIR");
    println!("cargo:rerun-if-changed=.");
    let (root, store) = repository(&crate_dir);
    let from_out = relative(&env_path("OUT_DIR"), &crate_dir);
    let source = store.and_then(|store| {
        let subtypes = subtypes(&root)?;
        set_crate_source(&crate_dir, &from_out, &mut Printed::new(&store, &subtypes))
    });
    write_out("cards.rs", source);
}

/// The shared crate's whole build script: writes `OUT_DIR/printed.rs`, the printed
/// characteristics of the tokens its modules define.
///
/// # Panics
///
/// With what is wrong, failing the build: [`shared_source`]'s errors.
pub fn shared() {
    let crate_dir = env_path("CARGO_MANIFEST_DIR");
    println!("cargo:rerun-if-changed=src");
    let (root, store) = repository(&crate_dir);
    let source = store.and_then(|store| {
        let subtypes = subtypes(&root)?;
        shared_source(&crate_dir.join("src"), &mut Printed::new(&store, &subtypes))
    });
    write_out("printed.rs", source);
}

/// The registry's whole build script: writes `OUT_DIR/tables.rs` and `OUT_DIR/names.rs`, and
/// runs again when a card file, a set crate or a decklist changes.
///
/// # Panics
///
/// With what is wrong, failing the build: [`registry_source`]'s errors, or a set crate that
/// the registry's manifest does not depend on with its `scenarios` feature passed on, the one
/// list Cargo makes a person keep.
pub fn registry() {
    let manifest_dir = env_path("CARGO_MANIFEST_DIR");
    let (root, store) = repository(&manifest_dir);
    let (cards_dir, decks_dir) = (root.join("rust/cards"), root.join("docs/sample-decks"));
    println!("cargo:rerun-if-changed=../../cards");
    println!("cargo:rerun-if-changed=../../../docs/sample-decks");
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("a manifest");
    for (name, _) in set_crates(&cards_dir) {
        assert!(
            manifest.contains(&format!("\n{name} = "))
                && manifest.contains(&format!("\"{name}/scenarios\"")),
            "rust/cards/{name} is a new set crate: add it to the dependencies of beryl_cards' \
             Cargo.toml and to its `scenarios` feature"
        );
    }
    let sources = store.and_then(|store| registry_source(&cards_dir, &decks_dir, &store));
    let (tables, names) = match sources {
        Ok((tables, names)) => (Ok(tables), Ok(names)),
        Err(error) => (Err(error.clone()), Err(error)),
    };
    write_out("tables.rs", tables);
    write_out("names.rs", names);
}
