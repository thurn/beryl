//! The sources the build scripts write to `OUT_DIR`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::catalogs::Catalogs;
use crate::complete::{self, Group, Keywords, complete_from_store};
use crate::files::{card_files, set_crates};
use crate::names::{camel, constant, constant_of_camel};
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

/// The CamelCase constants `text` reads from a generated type: each `<prefix><Camel>`, where
/// `prefix` is `Card::`, `Printed::` or `Token::` and no letter, digit or underscore comes before
/// it.
fn camels_used<'a>(text: &'a str, prefix: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices(prefix) {
        let before = text[..at].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') {
            continue;
        }
        let rest = &text[at + prefix.len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        if rest.starts_with(|c: char| c.is_ascii_uppercase()) {
            found.push(&rest[..end]);
        }
    }
    found
}

/// The `names` constant a `Card::<used>` or `Token::<used>` in a file means, checked to spell
/// back to `used`: `name_of` looks the constant up.
fn constant_of_used(
    used: &str,
    prefix: &str,
    name_of: impl Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    let spelled = constant_of_camel(used);
    let name = name_of(&spelled).map_err(|e| format!("{prefix}{used}: {e}"))?;
    if camel(&name)? != used {
        return Err(format!(
            "{prefix}{used}: the card store's {name:?} is spelled {prefix}{}",
            camel(&name)?
        ));
    }
    Ok(spelled)
}

/// A generated type's associated constants: `pub struct <ty>;` with a constant for each
/// `(camel, value)`, which CamelCase lets card files write like an enum's variants.
fn associated_source(
    doc: &str,
    ty: &str,
    value_type: &str,
    constants: &[(String, String)],
) -> String {
    let mut out = format!(
        "{doc}
#[derive(Clone, Copy, Debug)]
pub struct {ty};

         #[allow(non_upper_case_globals)]
impl {ty} {{
"
    );
    for (camel, value) in constants {
        writeln!(out, "    pub const {camel}: {value_type} = {value};").unwrap();
    }
    out.push_str(
        "}
",
    );
    out
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
    /// The `(constant, camel, source)` of each face, in order.
    faces: Vec<(String, String, String)>,
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
            if self.faces.iter().any(|(c, _, _)| *c == spelled) {
                return Err(format!("two faces of this crate give printed::{spelled}"));
            }
            self.faces.push((spelled, camel(&name)?, source));
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
        for used in camels_used(text, "Token::") {
            let spelled = constant_of_used(used, "Token::", |c| {
                self.store
                    .tokens_by_constant(c)
                    .map(|(name, _)| name.to_string())
            })
            .map_err(|e| format!("{}: {e}", path.display()))?;
            if !self.tokens.contains(&spelled) {
                self.tokens.push(spelled);
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
        for (spelled, _, source) in &self.faces {
            writeln!(out, "    pub const {spelled}: beryl_card::Face = {source};").unwrap();
        }
        out.push_str("    pub mod token {\n");
        let mut tokens = Vec::new();
        for spelled in &self.tokens {
            let source = self.token(spelled)?;
            writeln!(
                out,
                "        pub const {spelled}: beryl_card::Face = {source};"
            )
            .unwrap();
            let (name, _) = self.store.tokens_by_constant(spelled)?;
            tokens.push((camel(name)?, format!("printed::token::{spelled}")));
        }
        out.push_str("    }\n}\n\n");
        let faces: Vec<(String, String)> = self
            .faces
            .iter()
            .map(|(spelled, camel, _)| (camel.clone(), format!("printed::{spelled}")))
            .collect();
        out.push_str(&associated_source(
            "/// Each face's printed characteristics, as `Printed::CardName`: where a\n\
             /// card file starts the face.",
            "Printed",
            "beryl_card::Face",
            &faces,
        ));
        out.push('\n');
        out.push_str(&associated_source(
            "/// The printed characteristics of the tokens this crate's files define, as\n\
             /// `Token::Clue`.",
            "Token",
            "beryl_card::Face",
            &tokens,
        ));
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
/// The tables are statics, not consts. Rust evaluates a const again in every crate that reads
/// it, so with const tables the registry evaluated every card's scenarios a second time, on one
/// thread after all the set crates had finished: at 50,000 cards that was 85 s and 5.9 GiB of a
/// 145 s cold check (the scale rehearsal, `just scale`). A static is evaluated once, in its own
/// crate, and the registry reads it.
///
/// It also writes `STORE_CARDS`: a definition for each card first printed in one of `group`'s
/// sets that is complete from the store and has no file ([`complete`]), in a module `store`.
///
/// # Errors
///
/// When a file is no card of its set in the store, uses a constant that spells no card or
/// token, or types a printed characteristic, or a face's characteristics are none the engine
/// can hold.
pub fn set_crate_source(
    crate_dir: &Path,
    from_out: &str,
    group: &Group,
    printed: &mut Printed,
) -> Result<String, String> {
    let store = printed.store;
    let files = card_files(crate_dir);
    // This crate's own cards' names, and the other cards' names its files use.
    let mut constants = BTreeMap::new();
    let mut used_names = BTreeMap::new();
    let mut tree = String::new();
    let mut cards = String::new();
    let mut scenarios = String::new();
    let mut open: Option<&str> = None;
    let mut written = Vec::new();
    // The `Card` constants: the files' own cards, then every other card they name.
    let mut card_names: Vec<(String, String)> = Vec::new();
    for file in &files {
        let name = store.in_set(&file.set, &file.module)?;
        written.push(name);
        let own = constant(name)?;
        let text = std::fs::read_to_string(&file.path)
            .map_err(|e| format!("{}: {e}", file.path.display()))?;
        printed.card(&store.card(name)?)?;
        printed.file(&file.path, &text)?;
        for used in constants_used(&text, "names::") {
            if !used_names.contains_key(used) {
                let named = store
                    .by_constant(used)
                    .map_err(|e| format!("{}: {e}", file.path.display()))?;
                used_names.insert(used.to_string(), named.to_string());
            }
        }
        for used in camels_used(&text, "Card::") {
            let spelled =
                constant_of_used(used, "Card::", |c| store.by_constant(c).map(str::to_string))
                    .map_err(|e| format!("{}: {e}", file.path.display()))?;
            if let std::collections::btree_map::Entry::Vacant(entry) = used_names.entry(spelled) {
                let named = store.by_constant(entry.key())?;
                entry.insert(named.to_string());
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
        card_names.push((camel(name)?, format!("names::{own}")));
        constants.insert(own, name.to_string());
    }
    if open.is_some() {
        tree.push_str("}\n");
    }
    let (from_store, store_cards) = store_cards(group, &written, &mut constants, printed)?;
    for (spelled, name) in &used_names {
        let value = format!("names::{spelled}");
        if !card_names.iter().any(|(_, v)| *v == value) {
            card_names.push((camel(name)?, value));
        }
    }
    used_names.retain(|constant, _| !constants.contains_key(constant));
    let card_type = associated_source(
        "/// The card names this crate's files use, as `Card::CardName`: its own cards' and\n\
         /// the other cards' its files name.",
        "Card",
        "&str",
        &card_names,
    );
    Ok(format!(
        "// Generated by beryl_card_build from this crate's directories and the card store.\n\n\
         pub use beryl_card_shared::{{Prop, props, tokens}};\n\n\
         /// What a card file uses, `use crate::prelude::*;`: the vocabulary, this crate's\n\
         /// names and printed faces, and with the `scenarios` feature the scenario builder.\n\
         pub mod prelude {{\n    \
         pub use super::{{Card, Printed, Prop, Token}};\n    \
         pub use beryl_card::prelude::*;\n    \
         #[cfg(feature = \"scenarios\")]\n    \
         pub use beryl_scenario_types::prelude::*;\n}}\n\n\
         /// The names of this crate's cards: those with a file and those complete from the card\n\
         /// store. Each card is first printed in one set, so no two set crates share a name, and\n\
         /// the registry re-exports every set crate's as `beryl_cards::names`.\n\
         pub mod own_names {{\n{}}}\n\n\
         /// The names this crate's files use: its own cards' and the other cards' they name.\n\
         pub mod names {{\n    pub use super::own_names::*;\n{}}}\n\n\
         {card_type}\n\
         {}\n\
         {tree}\n\
         /// This crate's cards, by set and file.\n\
         pub static CARDS: [&beryl_card::CardDef; {count}] = [\n{cards}];\n\n\
         /// Each card's scenarios, by its name, in the order of `CARDS`.\n\
         #[cfg(feature = \"scenarios\")]\n\
         pub static SCENARIOS: [(&str, &[beryl_scenario_types::Scenario]); {count}] = [\n{scenarios}];\n\n\
         {from_store}\
         /// The cards of this crate's sets complete from the card store, which have no file.\n\
         pub static STORE_CARDS: [&beryl_card::CardDef; {store_count}] = [\n{store_cards}];\n",
        constants_source(&constants, "    "),
        constants_source(&used_names, "    "),
        printed.source()?,
        count = files.len(),
        store_count = store_cards.lines().count(),
    ))
}

/// The `store` module of a set crate and the entries of its `STORE_CARDS`: a definition for
/// each card of `group`'s sets complete from the store that is not among `written`, the cards
/// with files. Adds each card's name to `constants` and its face to `printed`.
fn store_cards(
    group: &Group,
    written: &[&str],
    constants: &mut BTreeMap<String, String>,
    printed: &mut Printed,
) -> Result<(String, String), String> {
    let store = printed.store;
    let (mut statics, mut table) = (String::new(), String::new());
    for name in store.first_printed_in(group.sets) {
        if written.contains(&name) {
            continue;
        }
        let record = store.card(name)?;
        // A face the engine cannot hold, such as a power that is no number, needs a file.
        let Some(keywords) = complete_from_store(&record, group.keywords)
            .filter(|_| faces(&record, printed.subtypes).is_ok())
        else {
            continue;
        };
        let own = constant(name)?;
        printed.card(&record)?;
        constants.insert(own.clone(), name.to_string());
        let face = complete::face_source(&own, &keywords);
        writeln!(
            statics,
            "    pub static {own}: beryl_card::CardDef = beryl_card::card(names::{own}, &[{face}]);"
        )
        .unwrap();
        writeln!(table, "    &store::{own},").unwrap();
    }
    if statics.is_empty() {
        return Ok((String::new(), table));
    }
    let module = format!(
        "/// The definitions of the cards of this crate's sets complete from the card store: each\n\
         /// its printed face with its keywords (`beryl_card_build`'s `complete`).\n\
         pub mod store {{\n    use super::{{names, printed}};\n\n{statics}}}\n\n"
    );
    Ok((module, table))
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

/// The `names::` constants the Rust files under `dir` use, read as text: the registry's own
/// sources, whose scenarios may name a card with no file, one complete from the store.
fn constants_in(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                found.extend(
                    constants_used(&text, "names::")
                        .into_iter()
                        .map(String::from),
                );
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// The registry's tables: their parts, one per set crate of `cards_dir`, in the crates' order.
pub fn registry_source(cards_dir: &Path) -> String {
    let crates = set_crates(cards_dir);
    let list = |table: &str| -> String {
        let mut out = String::new();
        for (name, _) in &crates {
            writeln!(out, "    &{name}::{table},").unwrap();
        }
        out
    };
    format!(
        "// Generated by beryl_card_build from the set crates under rust/cards.\n\n\
         /// Each set crate's cards, in the crates' order.\n\
         const SET_CARDS: &[&[&beryl_card::CardDef]] = &[\n{}];\n\n\
         /// Each set crate's scenarios by card name, in the crates' order.\n\
         #[cfg(feature = \"scenarios\")]\n\
         const SET_SCENARIOS: &[&[(&str, &[beryl_scenario_types::Scenario])]] = &[\n{}];\n\n\
         /// Each set crate's cards complete from the card store, in the crates' order.\n\
         const SET_STORE_CARDS: &[&[&beryl_card::CardDef]] = &[\n{}];\n",
        list("CARDS"),
        list("SCENARIOS"),
        list("STORE_CARDS"),
    )
}

/// The registry's `names` module: every set crate's own names (`own_names`, the cards with a
/// file and those complete from the card store), and as constants of its own the cards of the
/// decklists in `decks_dir`, the basic lands and each card the sources under `own_dir` name that
/// the store has, such as a card a library scenario plays. A constant of its own shadows a set
/// crate's of the same name, which spells the same card.
///
/// The set crates' names are re-exported rather than listed, so that this module changes only
/// when a set crate is added, a decklist changes or the registry's own sources do. Listing every
/// card with a file here made the build script watch `rust/cards`, and Cargo reran it, then
/// rechecked its 50,000 constants, after an edit to any card file (the scale rehearsal,
/// `just scale`).
///
/// # Errors
///
/// When a decklist cannot be read, or two names give one constant.
pub fn names_source(
    cards_dir: &Path,
    decks_dir: &Path,
    own_dir: &Path,
    store: &Store,
) -> Result<String, String> {
    let mut names = deck_names(decks_dir)?;
    names.extend(BASIC_LANDS.map(str::to_string));
    // A constant no card spells is left to the compiler, which names the file that uses it.
    for used in constants_in(own_dir) {
        if let Ok(name) = store.by_constant(&used) {
            names.push(name.to_string());
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
    let mut out = String::from(
        "// Generated by beryl_card_build from the set crates, docs/sample-decks and the card \
         store.\n\n",
    );
    // A set crate whose cards are all among the constants below re-exports nothing here, which
    // Rust reports as an unused import.
    for (name, _) in set_crates(cards_dir) {
        writeln!(
            out,
            "#[allow(unused_imports)]\npub use {name}::own_names::*;"
        )
        .unwrap();
    }
    out.push('\n');
    out.push_str(&constants_source(&constants, ""));
    Ok(out)
}

/// A path of the environment Cargo gives a build script.
pub(crate) fn env_path(variable: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} is not set")))
}

/// Writes `source` to `OUT_DIR/<file>`, or fails the build with the error.
pub(crate) fn write_out(file: &str, source: Result<String, String>) {
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

/// The subtypes the engine has, from the catalogs under `root`, telling Cargo to run the
/// script again when they change.
fn subtypes(root: &Path) -> Result<Subtypes, String> {
    Ok(Subtypes::of_catalogs(&Catalogs::read(root)?))
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
    println!("cargo:rerun-if-changed=../../../data/set-groups.json");
    let (root, store) = repository(&crate_dir);
    let from_out = relative(&env_path("OUT_DIR"), &crate_dir);
    let name = crate_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let source = store.and_then(|store| {
        let subtypes = subtypes(&root)?;
        let groups = root.join("data/set-groups.json");
        let groups =
            std::fs::read_to_string(&groups).map_err(|e| format!("{}: {e}", groups.display()))?;
        let sets = complete::group_sets(&groups, &name)?;
        let keywords = Keywords::read(&root)?;
        let group = Group {
            sets: &sets,
            keywords: &keywords,
        };
        set_crate_source(
            &crate_dir,
            &from_out,
            &group,
            &mut Printed::new(&store, &subtypes),
        )
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

/// The registry's whole build script: writes `OUT_DIR/tables.rs` and `OUT_DIR/names.rs`
/// ([`names_source`]), and runs again when a set crate is added, a decklist changes, the card
/// store changes or the registry's own sources do. No edit to a card file reruns it.
///
/// # Panics
///
/// When a set crate is one that the registry's manifest does not depend on with its
/// `scenarios` feature passed on, the one list Cargo makes a person keep; and with
/// [`names_source`]'s errors.
pub fn registry() {
    let manifest_dir = env_path("CARGO_MANIFEST_DIR");
    let (root, store) = repository(&manifest_dir);
    let cards_dir = root.join("rust/cards");
    // A new set crate is a new workspace member, which Cargo writes into the lock file before it
    // builds. Watching `rust/cards` instead would make Cargo look at every card file's time
    // before every build, 50,000 of them in the scale rehearsal (`just scale`).
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=../../../docs/sample-decks");
    println!("cargo:rerun-if-changed=src");
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("a manifest");
    for (name, _) in set_crates(&cards_dir) {
        assert!(
            manifest.contains(&format!("\n{name} = "))
                && manifest.contains(&format!("\"{name}/scenarios\"")),
            "rust/cards/{name} is a new set crate: add it to the dependencies of beryl_cards' \
             Cargo.toml and to its `scenarios` feature"
        );
    }
    write_out("tables.rs", Ok(registry_source(&cards_dir)));
    let names = store.and_then(|store| {
        names_source(
            &cards_dir,
            &root.join("docs/sample-decks"),
            &manifest_dir.join("src"),
            &store,
        )
    });
    write_out("names.rs", names);
}
