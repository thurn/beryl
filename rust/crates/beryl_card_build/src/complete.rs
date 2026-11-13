//! The cards complete from the card store: a pool card that needs no file, because its record
//! says all there is to it (plan K2: "keyword-only creatures need no authoring"; the run epic:
//! "an unimplemented card is a record in the card store, never a file").
//!
//! Such a card is a single-faced creature whose Oracle text, with its reminder text removed,
//! is empty or only keyword abilities the engine has. The engine's keywords are the
//! constants of `Keywords` in `beryl_card`'s `characteristics.rs`, read from that source, so the
//! rule follows the list as cards add keywords and no second list is kept. Its definition is
//! its `printed::` face with those keywords and nothing else, which the set crate's build
//! script writes ([`crate::set_crate_source`]).
//!
//! A card with a file is not generated: the file replaces the definition the store would give.
//! A file may exist for such a card, to add what the store lacks or because it was written
//! before the engine had its keywords (AGENTS.md: "Never delete a card implementation").

use std::fmt::Write as _;
use std::path::Path;

use beryl_card_store::Record;

/// The engine's reader of mana cost notation, which `beryl_card::mana` builds a cost with: a
/// printed cost it refuses, such as a hybrid one, keeps a card from being complete.
#[path = "../../beryl_card/src/mana_notation.rs"]
mod mana_notation;

/// Where `Keywords` is defined, from the repository's root.
pub const KEYWORDS_SOURCE: &str = "rust/crates/beryl_card/src/characteristics.rs";

/// The keyword abilities the engine has, each as Oracle text spells it in lower case with its
/// constant of `beryl_card::Keywords`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keywords(Vec<(String, String)>);

impl Keywords {
    /// Reads the constants of the `Keywords` bitset from `beryl_card`'s source: each line
    /// `NAME = bit,` between `Keywords(u64) {` and the brace that closes it. A constant's name
    /// spells the keyword: `FIRST_STRIKE` is "first strike".
    ///
    /// # Errors
    ///
    /// When the source has no `Keywords` bitset, or one with no constants.
    pub fn parse(source: &str) -> Result<Self, String> {
        let start = source
            .find("Keywords(u64) {")
            .ok_or(format!("{KEYWORDS_SOURCE}: no `Keywords(u64)` bitset"))?;
        let mut found = Vec::new();
        for line in source[start..].lines().skip(1) {
            let line = line.trim();
            if line == "}" {
                break;
            }
            let Some((constant, bit)) = line.split_once(" = ") else {
                continue;
            };
            if bit.trim_end_matches(',').parse::<u8>().is_ok()
                && constant
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            {
                let spelled = constant.to_ascii_lowercase().replace('_', " ");
                found.push((spelled, constant.to_string()));
            }
        }
        if found.is_empty() {
            return Err(format!("{KEYWORDS_SOURCE}: `Keywords` has no constants"));
        }
        Ok(Self(found))
    }

    /// Reads `Keywords` from the source under `root`, telling Cargo to run the script again
    /// when it changes.
    ///
    /// # Errors
    ///
    /// When the file cannot be read, or as [`Keywords::parse`].
    pub fn read(root: &Path) -> Result<Self, String> {
        println!("cargo:rerun-if-changed=../../../{KEYWORDS_SOURCE}");
        let path = root.join(KEYWORDS_SOURCE);
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&source)
    }

    /// The constant of the keyword Oracle text spells as `words`, in any case.
    fn constant(&self, words: &str) -> Option<&str> {
        let words = words.to_lowercase();
        self.0
            .iter()
            .find(|(spelled, _)| *spelled == words)
            .map(|(_, constant)| constant.as_str())
    }
}

/// What a set crate's build needs to write its cards complete from the store: the sets of its
/// group in `data/set-groups.json` and the engine's keywords.
pub struct Group<'a> {
    pub sets: &'a [String],
    pub keywords: &'a Keywords,
}

impl Group<'_> {
    /// A group of no sets, which writes no card from the store.
    pub const NONE: Group<'static> = Group {
        sets: &[],
        keywords: &Keywords(Vec::new()),
    };
}

/// The sets of the group `name` in the text of `data/set-groups.json`.
///
/// # Errors
///
/// When the text is no set-groups file, or has no group of that name.
pub fn group_sets(text: &str, name: &str) -> Result<Vec<String>, String> {
    let groups: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("data/set-groups.json: {e}"))?;
    let group = groups["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|g| g["crate"] == name)
        .ok_or(format!("data/set-groups.json has no group {name}"))?;
    Ok(group["sets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|set| set.as_str().map(str::to_string))
        .collect())
}

/// `text` without its reminder text, which is in parentheses
/// ([CR 207.2a](https://yawgatog.com/resources/magic-rules/#R2072a)): "Reminder text is
/// italicized text within parentheses that summarizes a rule that applies to that card."
fn without_reminders(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The keyword constants of `record` when the card is complete from the store: a card (not a
/// token) of one face and the `normal` layout, a creature with a mana cost the engine can
/// hold, whose Oracle text without its reminder text is empty or a list of keywords
/// `keywords` has, one per line or separated by commas. None when the card needs a file.
#[must_use]
pub fn complete_from_store<'k>(record: &Record, keywords: &'k Keywords) -> Option<Vec<&'k str>> {
    let [face] = &record.faces[..] else {
        return None;
    };
    let card_types = face.type_line.split(" — ").next().unwrap_or_default();
    if record.first_set.is_none()
        || record.layout != "normal"
        || !card_types.split(' ').any(|word| word == "Creature")
        || mana_notation::read(face.mana_cost.as_deref().unwrap_or_default()).is_err()
    {
        return None;
    }
    let text = without_reminders(face.oracle_text.as_deref().unwrap_or_default());
    let mut found: Vec<&str> = Vec::new();
    for words in text.split(['\n', ',']).map(str::trim) {
        if words.is_empty() {
            continue;
        }
        let constant = keywords.constant(words)?;
        if !found.contains(&constant) {
            found.push(constant);
        }
    }
    Some(found)
}

/// The face source of a card complete from the store: its `printed::` constant with its
/// keywords.
#[must_use]
pub fn face_source(spelled: &str, constants: &[&str]) -> String {
    let mut out = format!("printed::{spelled}");
    let mut flags = constants
        .iter()
        .map(|constant| format!("beryl_card::Keywords::{constant}"));
    if let Some(first) = flags.next() {
        let all = flags.fold(first, |all, next| format!("{all}.union({next})"));
        write!(out, ".keywords({all})").unwrap();
    }
    out
}
