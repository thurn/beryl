//! The vocabulary's lists generated from Scryfall's catalogs, `data/catalogs.json` (plan K2:
//! "Subtypes, counter kinds and similar lists are generated from Scryfall's catalogs, not
//! written by hand"): `beryl_card`'s `Subtype` and its `counter_kinds!` list, which its build
//! script writes with [`vocabulary`], and the subtypes a printed face may have ([`Subtypes`]).
//! `tools/src/beryl_tools/catalogs.py` writes the file.

use std::fmt::Write;
use std::path::Path;

/// The subtype catalogs, each with the card types whose subtypes it lists
/// ([CR 205.3c](https://yawgatog.com/resources/magic-rules/#R2053c): "If a card with multiple
/// card types has one or more subtypes, each subtype is correlated to its appropriate card
/// type."), as `beryl_card::Types` flags. Instants and sorceries share the spell types
/// ([CR 205.3k](https://yawgatog.com/resources/magic-rules/#R2053k)), and creatures and
/// kindreds the creature types
/// ([CR 205.3m](https://yawgatog.com/resources/magic-rules/#R2053m): "Creatures and kindreds
/// share their lists of subtypes; these subtypes are called creature types."). The order is
/// `Subtype`'s, so a catalog is a run of its variants.
const SUBTYPE_CATALOGS: [(&str, &str); 7] = [
    ("land-types", "LAND"),
    ("artifact-types", "ARTIFACT"),
    ("enchantment-types", "ENCHANTMENT"),
    ("spell-types", "INSTANT.union(Types::SORCERY)"),
    ("planeswalker-types", "PLANESWALKER"),
    ("battle-types", "BATTLE"),
    ("creature-types", "CREATURE.union(Types::KINDRED)"),
];

/// The variants the counter kinds kept from the list written by hand, which no spelling rule
/// gives: the rules call these "+1/+1" and "-1/-1" counters.
const COUNTER_VARIANTS: [(&str, &str); 2] = [("+1/+1", "PlusOne"), ("-1/-1", "MinusOne")];

/// A subtype: its name as the rules print it, its variant and its catalog's card types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subtype {
    pub name: String,
    pub variant: String,
    pub card_types: &'static str,
}

/// The catalogs the vocabulary is written from.
pub struct Catalogs {
    /// In `Subtype`'s order: by catalog as [`SUBTYPE_CATALOGS`] orders them, then by name.
    pub subtypes: Vec<Subtype>,
    /// The counter kinds as `(name, variant)`, sorted by name.
    pub counter_kinds: Vec<(String, String)>,
    /// The card types and supertypes the catalogs list.
    pub card_types: Vec<String>,
    pub supertypes: Vec<String>,
}

/// A name spelled as a variant: its letters and digits, each word capitalized, so
/// "Time Lord" is `TimeLord`, "Assembly-Worker" `AssemblyWorker`, "Urza's" `Urzas` and
/// "first strike" `FirstStrike`.
#[must_use]
pub fn variant(name: &str) -> String {
    let mut out = String::new();
    for word in name.split([' ', '-']) {
        let mut letters = word.chars().filter(char::is_ascii_alphanumeric);
        if let Some(first) = letters.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(letters);
        }
    }
    out
}

/// The numbers a power and toughness counter kind's variant spells.
const NUMBERS: [&str; 10] = [
    "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
];

/// A counter kind spelled as a variant: a power and toughness kind ("+1/+2") by its signs and
/// numbers in words, `PlusOnePlusTwo`; any other as [`variant`] spells it.
fn counter_variant(name: &str) -> Result<String, String> {
    if let Some((_, kept)) = COUNTER_VARIANTS.iter().find(|(n, _)| *n == name) {
        return Ok((*kept).to_string());
    }
    let Some((power, toughness)) = name.split_once('/') else {
        let spelled = variant(name);
        return match spelled.chars().next() {
            Some(c) if c.is_ascii_alphabetic() => Ok(spelled),
            _ => Err(format!(
                "catalogs.json: no variant spells the counter kind {name:?}"
            )),
        };
    };
    let mut out = String::new();
    for part in [power, toughness] {
        let (sign, number) = part.split_at(1);
        out.push_str(match sign {
            "+" => "Plus",
            "-" => "Minus",
            _ => return Err(format!("catalogs.json: counter kind {name:?} has no sign")),
        });
        let n: usize = number
            .parse()
            .ok()
            .filter(|&n| n < NUMBERS.len())
            .ok_or(format!(
                "catalogs.json: counter kind {name:?} has no small number"
            ))?;
        out.push_str(NUMBERS[n]);
    }
    Ok(out)
}

impl Catalogs {
    /// Reads the catalogs file's text.
    ///
    /// # Errors
    ///
    /// When the text is no catalogs file, a catalog is missing, or two entries spell one
    /// variant.
    pub fn parse(text: &str) -> Result<Self, String> {
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("catalogs.json: {e}"))?;
        let list = |value: &serde_json::Value, name: &str| -> Result<Vec<String>, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_array)
                .ok_or(format!("catalogs.json has no list {name}"))?
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .ok_or(format!(
                    "catalogs.json: {name} holds a value that is no string"
                ))
        };
        let catalogs = json
            .get("catalogs")
            .ok_or("catalogs.json has no catalogs")?;
        let mut subtypes = Vec::new();
        for (catalog, card_types) in SUBTYPE_CATALOGS {
            let mut names = list(catalogs, catalog)?;
            names.sort();
            subtypes.extend(names.into_iter().map(|name| Subtype {
                variant: variant(&name),
                name,
                card_types,
            }));
        }
        let mut counter_kinds = list(&json, "counter_kinds")?
            .into_iter()
            .map(|name| Ok((counter_variant(&name)?, name)))
            .map(|r: Result<_, String>| r.map(|(v, n)| (n, v)))
            .collect::<Result<Vec<_>, _>>()?;
        counter_kinds.sort();
        let catalogs = Catalogs {
            subtypes,
            counter_kinds,
            card_types: list(catalogs, "card-types")?,
            supertypes: list(catalogs, "supertypes")?,
        };
        catalogs.check_unique()?;
        Ok(catalogs)
    }

    /// Reads `data/catalogs.json` under `root`, telling Cargo to run the build script again
    /// when it changes.
    ///
    /// # Errors
    ///
    /// When the file cannot be read, or as [`Catalogs::parse`].
    pub fn read(root: &Path) -> Result<Self, String> {
        println!("cargo:rerun-if-changed=../../../data/catalogs.json");
        let path = root.join("data/catalogs.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    fn check_unique(&self) -> Result<(), String> {
        let mut subtypes: Vec<&str> = self.subtypes.iter().map(|s| s.variant.as_str()).collect();
        let mut kinds: Vec<&str> = self.counter_kinds.iter().map(|(_, v)| v.as_str()).collect();
        for (what, list) in [("subtypes", &mut subtypes), ("counter kinds", &mut kinds)] {
            list.sort_unstable();
            let twice = list.windows(2).find_map(|pair| match pair {
                [a, b] if a == b => Some(*a),
                _ => None,
            });
            if let Some(twice) = twice {
                return Err(format!(
                    "catalogs.json: two {what} spell the variant {twice}"
                ));
            }
        }
        Ok(())
    }

    /// The variant of the subtype a type line spells as `word`.
    #[must_use]
    pub fn subtype_variant(&self, word: &str) -> Option<&str> {
        self.subtypes
            .iter()
            .find(|s| s.name == word)
            .map(|s| s.variant.as_str())
    }

    /// The source of `beryl_card`'s `Subtype`: the enum, `ALL`, each one's name and card
    /// types, and the words a set of them takes.
    #[must_use]
    pub fn subtype_source(&self) -> String {
        let mut out = String::from(
            "/// A subtype ([CR 205.3](https://yawgatog.com/resources/magic-rules/#R2053)), one\n\
             /// for each entry of Scryfall's subtype catalogs (`data/catalogs.json`), grouped by\n\
             /// the card types it belongs to.\n\
             #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n\
             #[repr(u16)]\n\
             pub enum Subtype {\n",
        );
        for s in &self.subtypes {
            writeln!(out, "    {},", s.variant).unwrap();
        }
        let n = self.subtypes.len();
        writeln!(
            out,
            "}}\n\n/// The 64-bit words a set of every subtype takes."
        )
        .unwrap();
        writeln!(
            out,
            "pub const SUBTYPE_WORDS: usize = {};\n",
            n.div_ceil(64)
        )
        .unwrap();
        writeln!(out, "impl Subtype {{\n    /// Every subtype, in order.").unwrap();
        writeln!(out, "    pub const ALL: [Subtype; {n}] = [").unwrap();
        for s in &self.subtypes {
            writeln!(out, "        Subtype::{},", s.variant).unwrap();
        }
        out.push_str(
            "    ];\n\n    /// Its name as a type line prints it.\n    \
             pub const fn name(self) -> &'static str {\n        \
             SUBTYPE_NAMES[self as usize]\n    }\n\n    \
             /// The card types it belongs to (CR 205.3c).\n    \
             pub const fn card_type(self) -> Types {\n        \
             match self as u16 {\n",
        );
        let mut start = 0;
        for (catalog, card_types) in SUBTYPE_CATALOGS {
            let end = start
                + self
                    .subtypes
                    .iter()
                    .filter(|s| s.card_types == card_types)
                    .count();
            if end > start {
                let arm = if end == n {
                    "_".to_string()
                } else {
                    format!("{start}..{end}")
                };
                writeln!(out, "            // {catalog}").unwrap();
                writeln!(out, "            {arm} => Types::{card_types},").unwrap();
            }
            start = end;
        }
        out.push_str("        }\n    }\n}\n\n/// Each subtype's name, by [`Subtype`].\n");
        writeln!(out, "const SUBTYPE_NAMES: [&str; {n}] = [").unwrap();
        for s in &self.subtypes {
            writeln!(out, "    {:?},", s.name).unwrap();
        }
        out.push_str("];\n");
        out
    }

    /// The source of `beryl_card`'s `counter_kinds!` macro: the list of every counter kind,
    /// handed to the macro it names as `Variant = "name"` entries.
    #[must_use]
    pub fn counter_kinds_source(&self) -> String {
        let mut out = String::from(
            "/// The kinds of counter, each with its name in the rules\n\
             /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)), one for each\n\
             /// entry of the catalogs' counter list (`data/catalogs.json`), sorted by name. It\n\
             /// hands its entries to the macro named, so a table elsewhere is one more macro\n\
             /// over the same entries. A `const fn` cannot enumerate an enum's variants.\n\
             #[macro_export]\n\
             macro_rules! counter_kinds {\n    ($with:ident) => {\n        $with! {\n",
        );
        for (name, variant) in &self.counter_kinds {
            writeln!(out, "            {variant} = {name:?},").unwrap();
        }
        out.push_str("        }\n    };\n}\n");
        out
    }
}

/// `beryl_card`'s whole build script: writes `OUT_DIR/subtypes.rs` and
/// `OUT_DIR/counter_kinds.rs` from the catalogs, which its modules include.
///
/// # Panics
///
/// With what is wrong, failing the build: [`Catalogs::read`]'s errors.
pub fn vocabulary() {
    let crate_dir = crate::generate::env_path("CARGO_MANIFEST_DIR");
    let catalogs = Catalogs::read(&crate_dir.join("../../.."));
    crate::generate::write_out(
        "subtypes.rs",
        catalogs
            .as_ref()
            .map(Catalogs::subtype_source)
            .map_err(Clone::clone),
    );
    crate::generate::write_out(
        "counter_kinds.rs",
        catalogs.map(|c| c.counter_kinds_source()),
    );
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use beryl_card_store::Record;

    use super::{Catalogs, counter_variant, variant};
    use crate::printed::{SUPERTYPES, Subtypes, TYPES, TypeLine};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    fn catalogs() -> Catalogs {
        Catalogs::read(&root()).unwrap()
    }

    /// A variant spells a name's words, and a power and toughness kind its numbers; the kinds
    /// the rules read keep their names.
    #[test]
    fn names_spell_variants() {
        assert_eq!(variant("Time Lord"), "TimeLord");
        assert_eq!(variant("Assembly-Worker"), "AssemblyWorker");
        assert_eq!(variant("Urza's"), "Urzas");
        assert_eq!(variant("B.O.B."), "BOB");
        assert_eq!(counter_variant("first strike").unwrap(), "FirstStrike");
        assert_eq!(counter_variant("+1/+2").unwrap(), "PlusOnePlusTwo");
        assert_eq!(counter_variant("-0/-1").unwrap(), "MinusZeroMinusOne");
        assert_eq!(counter_variant("+1/+1").unwrap(), "PlusOne");
        assert_eq!(counter_variant("-1/-1").unwrap(), "MinusOne");
        assert!(counter_variant("+X/+X").is_err());
    }

    /// Every card type and supertype the engine has is one the catalogs list
    /// ([CR 300.1](https://yawgatog.com/resources/magic-rules/#R3001),
    /// [CR 205.4a](https://yawgatog.com/resources/magic-rules/#R2054a)), and the counter
    /// list keeps the kinds the rules read.
    #[test]
    fn the_engine_types_are_the_catalogs() {
        let catalogs = catalogs();
        for (name, _) in TYPES {
            assert!(catalogs.card_types.iter().any(|t| t == name), "{name}");
        }
        for (name, _) in SUPERTYPES {
            assert!(catalogs.supertypes.iter().any(|t| t == name), "{name}");
        }
        for kind in ["+1/+1", "-1/-1", "loyalty", "time", "story", "chorus"] {
            assert!(
                catalogs.counter_kinds.iter().any(|(n, _)| n == kind),
                "{kind}"
            );
        }
    }

    /// Every type line of every card and token in the pool reads as the engine's supertypes,
    /// card types and generated subtypes, so the store's whole pool can be generated. A
    /// double-faced token's reminder face, "Card", is no object and has no type.
    #[test]
    fn every_pool_type_line_reads() {
        let subtypes = Subtypes::of_catalogs(&catalogs());
        let text = std::fs::read_to_string(root().join("data/card-store.tsv")).unwrap();
        let mut read = 0;
        let mut failures = Vec::new();
        for line in text.lines() {
            let Some((key, json)) = line.split_once('\t') else {
                continue;
            };
            // An alias names a record; it is none.
            if !(key.starts_with("card:") || key.starts_with("token:"))
                || json.starts_with("{\"alias")
            {
                continue;
            }
            let record: Record = serde_json::from_str(json).unwrap();
            for face in record.faces.iter().filter(|f| f.type_line != "Card") {
                match TypeLine::parse(&face.type_line, &subtypes) {
                    Ok(_) => read += 1,
                    Err(e) => failures.push(format!("{}: {e}", face.name)),
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(read > 30_000, "{read}");
    }
}
