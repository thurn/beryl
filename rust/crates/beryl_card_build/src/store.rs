//! The card store, `data/card-store.tsv`: its card names, each with its first-printing set,
//! and its records. `tools/src/beryl_tools/card_store.py` specifies the format; this reads
//! the names with the standard library alone, and a record with `beryl_card_store`'s reader.

use std::collections::HashMap;
use std::path::Path;

pub use beryl_card_store::Record;

/// A name the store answers to, a card's own or one of its faces', with the set the card was
/// first printed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub name: String,
    pub set: String,
}

/// The cards of the store, found by a file's modules or by a constant, and its records.
pub struct Store {
    /// By [`crate::names::card_module`] of the name.
    by_module: HashMap<String, Vec<Card>>,
    /// Each card's own name, not a face's, with its first set, sorted by name.
    cards: Vec<Card>,
    /// The names of its tokens and emblems, by [`crate::names::card_module`] of each.
    tokens_by_module: HashMap<String, Vec<String>>,
    /// The store's text, which [`beryl_card_store::Store`] searches for a record.
    text: String,
}

/// The JSON string starting after `"<field>":"` in `value`, unescaped. The store's values are
/// compact JSON written with `ensure_ascii=False`, so a string escapes only `"`, `\` and
/// control characters.
fn string_field(value: &str, field: &str) -> Option<String> {
    let start = value.find(&format!("\"{field}\":\""))? + field.len() + 4;
    let mut out = String::new();
    let mut chars = value[start..].chars();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let code: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?)?);
                }
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
}

impl Store {
    /// Reads the store's text.
    ///
    /// # Errors
    ///
    /// When a card's name cannot be spelled as a module, or an alias names no record.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut sets: HashMap<&str, String> = HashMap::new();
        let mut aliases: Vec<(&str, String)> = Vec::new();
        let mut tokens_by_module: HashMap<String, Vec<String>> = HashMap::new();
        for line in text.lines() {
            if let Some((name, _)) = line
                .strip_prefix("token:")
                .and_then(|entry| entry.split_once('\t'))
            {
                let spelled = tokens_by_module
                    .entry(crate::names::card_module(name)?)
                    .or_default();
                if !spelled.iter().any(|n| n == name) {
                    spelled.push(name.to_string());
                }
                continue;
            }
            let Some((name, value)) = line
                .strip_prefix("card:")
                .and_then(|entry| entry.split_once('\t'))
            else {
                continue;
            };
            if value.starts_with("{\"alias\":") {
                let target = string_field(value, "alias");
                aliases.push((name, target.ok_or(format!("{name}: an unreadable alias"))?));
            } else {
                let set = string_field(value, "first_set");
                sets.insert(name, set.ok_or(format!("{name}: no first_set"))?);
            }
        }
        let mut by_module: HashMap<String, Vec<Card>> = HashMap::new();
        let mut add = |name: &str, set: &str| -> Result<(), String> {
            let card = Card {
                name: name.to_string(),
                set: set.to_string(),
            };
            let cards = by_module
                .entry(crate::names::card_module(name)?)
                .or_default();
            if !cards.contains(&card) {
                cards.push(card);
            }
            Ok(())
        };
        for (&name, set) in &sets {
            add(name, set)?;
        }
        for (name, target) in &aliases {
            let set = sets.get(target.as_str());
            add(
                name,
                set.ok_or(format!("{name}: an alias of {target}, which is no card"))?,
            )?;
        }
        let mut cards: Vec<Card> = sets
            .iter()
            .map(|(&name, set)| Card {
                name: name.to_string(),
                set: set.clone(),
            })
            .collect();
        cards.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(Self {
            by_module,
            cards,
            tokens_by_module,
            text: text.to_string(),
        })
    }

    /// Reads the store at `path`.
    ///
    /// # Errors
    ///
    /// As [`Store::parse`], or when the file cannot be read.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    /// The name of the card whose file is `<set>/<module>.rs`.
    ///
    /// # Errors
    ///
    /// When no card first printed in that set, or more than one, is spelled `module`.
    pub fn in_set(&self, set: &str, module: &str) -> Result<&str, String> {
        let spelled = self.by_module.get(module).map_or(&[][..], Vec::as_slice);
        let found: Vec<&Card> = spelled
            .iter()
            .filter(|card| crate::names::set_module(&card.set) == set)
            .collect();
        match found[..] {
            [card] => Ok(&card.name),
            [] if spelled.is_empty() => Err(format!(
                "{set}/{module}.rs: no card in the store is spelled {module}"
            )),
            [] => Err(format!(
                "{set}/{module}.rs: {} was first printed in {}, not {set}: `just card path` \
                 prints where its file belongs",
                spelled[0].name, spelled[0].set
            )),
            _ => Err(format!(
                "{set}/{module}.rs: more than one card of {set} is spelled {module}"
            )),
        }
    }

    /// The cards first printed in one of `sets`, by name: each card's own name, not its faces'.
    pub fn first_printed_in<'s>(&'s self, sets: &'s [String]) -> impl Iterator<Item = &'s str> {
        self.cards
            .iter()
            .filter(|card| sets.contains(&card.set))
            .map(|card| card.name.as_str())
    }

    /// The name a `names::<constant>` means.
    ///
    /// # Errors
    ///
    /// When no card's name, or more than one's, is spelled `constant`.
    pub fn by_constant(&self, constant: &str) -> Result<&str, String> {
        let spelled = self.by_module.get(&constant.to_ascii_lowercase());
        let mut names: Vec<&str> = spelled
            .into_iter()
            .flatten()
            .map(|c| c.name.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        match names[..] {
            [name] => Ok(name),
            [] => Err(format!(
                "names::{constant}: no card in the store is spelled so"
            )),
            _ => Err(format!("names::{constant} could be any of {names:?}")),
        }
    }

    /// The record of the card named `name`, or with a face of that name.
    ///
    /// # Errors
    ///
    /// When the store has none.
    pub fn card(&self, name: &str) -> Result<Record, String> {
        beryl_card_store::Store::new(self.text.as_bytes())
            .card(name)
            .ok_or(format!("{name}: no record in the card store"))
    }

    /// The token or emblem a `printed::token::<constant>` means, and each of its records.
    ///
    /// # Errors
    ///
    /// When no token's name, or more than one's, is spelled `constant`.
    pub fn tokens_by_constant(&self, constant: &str) -> Result<(&str, Vec<Record>), String> {
        let spelled = self.tokens_by_module.get(&constant.to_ascii_lowercase());
        match spelled.map_or(&[][..], Vec::as_slice) {
            [name] => {
                let records = beryl_card_store::Store::new(self.text.as_bytes()).tokens(name);
                Ok((name, records))
            }
            [] => Err(format!(
                "printed::token::{constant}: no token in the store is spelled so"
            )),
            names => Err(format!(
                "printed::token::{constant} could be any of {names:?}"
            )),
        }
    }
}
