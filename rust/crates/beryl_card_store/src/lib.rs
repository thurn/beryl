//! The card store, `data/card-store.tsv`: a record for every card in the pool and every token
//! or emblem a pool card makes, which `just cards` writes from Scryfall's bulk data. The
//! format's specification is the header of `tools/src/beryl_tools/card_store.py`; in short,
//! lines of `<key>\t<json>` sorted by their bytes, keys `card:<name>` and `token:<name>`, and
//! alias values (`{"alias", "scryfall_id"}`) that send a face's name to its card's record.
//!
//! [`STORE`] embeds the file in the binary, so the desktop app, the phone apps, `beryl_cli`'s
//! servers and the tests all read it alike, with nothing bundled as a resource and nothing read
//! or parsed at startup. A lookup binary-searches the bytes: it probes about log2 of the
//! store's size in lines and parses only the records it returns. Name search uses a separate
//! lowercase index, built on first use.

use std::sync::LazyLock;

use serde::Deserialize;

pub mod facts;

mod search;
use search::NameIndex;

const STORE_BYTES: &[u8] = include_bytes!("../../../../data/card-store.tsv");
static STORE_NAMES: LazyLock<NameIndex<'static>> = LazyLock::new(|| NameIndex::new(STORE_BYTES));

/// The committed card store.
pub static STORE: Store<'static> = Store {
    bytes: STORE_BYTES,
    names: Some(&STORE_NAMES),
};

/// A card store's bytes.
#[derive(Clone, Copy, Debug)]
pub struct Store<'a> {
    bytes: &'a [u8],
    names: Option<&'a LazyLock<NameIndex<'a>>>,
}

/// A card, token or emblem: the printing shown and its faces' characteristics.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Record {
    pub name: String,
    pub oracle_id: String,
    pub scryfall_id: String,
    pub set: String,
    pub collector_number: String,
    /// A card's first printing's set code and release date (`YYYY-MM-DD`), which place its
    /// file; none for a token.
    pub first_set: Option<String>,
    pub first_released: Option<String>,
    pub layout: String,
    #[serde(default)]
    pub artist: String,
    /// The image of its back; none for a card whose back is its other face.
    pub card_back_id: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The tokens and emblems a card makes, each the key and Scryfall id of its record.
    #[serde(default)]
    pub tokens: Vec<TokenLink>,
    pub faces: Vec<Face>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct TokenLink {
    pub name: String,
    pub scryfall_id: String,
}

/// A face's characteristics, each present when Scryfall gives it, and the side of the printing
/// its image is on.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Face {
    pub name: String,
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: String,
    pub oracle_text: Option<String>,
    #[serde(default)]
    pub colors: Vec<String>,
    pub color_indicator: Option<Vec<String>>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub defense: Option<String>,
    pub side: Side,
}

/// The side of a printing a face's image is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Front,
    Back,
}

impl Side {
    /// Its name in a Scryfall image path.
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
        }
    }
}

/// A value in the store: a record, or another key's record answering this key too.
#[derive(Deserialize)]
#[serde(untagged)]
enum Value {
    Alias { alias: String, scryfall_id: String },
    Record(Box<Record>),
}

#[derive(Clone, Copy)]
enum Kind {
    Card,
    Token,
}

impl Kind {
    fn key(self, name: &str) -> String {
        match self {
            Kind::Card => format!("card:{name}"),
            Kind::Token => format!("token:{name}"),
        }
    }
}

impl<'a> Store<'a> {
    /// A store over `bytes`, which are read only when looked up.
    pub const fn new(bytes: &'a [u8]) -> Store<'a> {
        Store { bytes, names: None }
    }

    /// The card named `name`, or with a face of that name.
    pub fn card(&self, name: &str) -> Option<Record> {
        self.records(Kind::Card, name).into_iter().next()
    }

    /// The tokens and emblems named `name`, each printing the store has, in its order.
    pub fn tokens(&self, name: &str) -> Vec<Record> {
        self.records(Kind::Token, name)
    }

    fn records(&self, kind: Kind, name: &str) -> Vec<Record> {
        let mut out = Vec::new();
        for value in self.values(&kind.key(name)).0 {
            match parse(value) {
                Value::Record(record) => out.push(*record),
                Value::Alias { alias, scryfall_id } => out.extend(
                    self.values(&kind.key(&alias))
                        .0
                        .into_iter()
                        .filter_map(|v| match parse(v) {
                            Value::Record(r) if r.scryfall_id == scryfall_id => Some(*r),
                            _ => None,
                        }),
                ),
            }
        }
        out
    }

    /// The values under `key`, and how many lines the search probed to find them.
    fn values(&self, key: &str) -> (Vec<&'a str>, usize) {
        let target = key.as_bytes();
        let (mut low, mut high, mut probes) = (0, self.bytes.len(), 0);
        // The least offset whose next line's key is at or past the target.
        while low < high {
            let middle = low + (high - low) / 2;
            let start = self.line_from(middle);
            probes += 1;
            if start < self.bytes.len() && self.key_at(start) < target {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let mut start = self.line_from(low);
        let mut out = Vec::new();
        while start < self.bytes.len() && self.key_at(start) == target {
            let end = self.line_end(start);
            let value = &self.bytes[start + target.len() + 1..end];
            out.push(std::str::from_utf8(value).expect("the card store is UTF-8"));
            start = end + 1;
        }
        (out, probes)
    }

    /// The offset of the first line starting at or after `offset`.
    fn line_from(&self, offset: usize) -> usize {
        if offset == 0 {
            0
        } else {
            (self.line_end(offset - 1) + 1).min(self.bytes.len())
        }
    }

    /// The offset of the newline ending the line through `offset`, or the store's end.
    fn line_end(&self, offset: usize) -> usize {
        self.bytes[offset..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(self.bytes.len(), |i| offset + i)
    }

    /// The key of the line starting at `start`.
    fn key_at(&self, start: usize) -> &'a [u8] {
        let rest = &self.bytes[start..];
        let end = rest
            .iter()
            .position(|&b| b == b'\t' || b == b'\n')
            .unwrap_or(rest.len());
        &rest[..end]
    }
}

fn parse(value: &str) -> Value {
    serde_json::from_str(value).expect("a card store value is a record or an alias")
}

#[cfg(test)]
mod tests;
