//! Manual deck lists, import, and local storage. Import does not check deck legality.

use std::collections::HashSet;
use std::{fmt, io};

use beryl_card_store::Store;
use serde::{Deserialize, Serialize};

mod cod;
mod storage;

pub use cod::parse_cod;
pub use storage::Decks;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Deck {
    pub name: String,
    pub main: Vec<(String, u16)>,
    pub side: Vec<(String, u16)>,
    /// Nonempty text lines that could not be read as entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

#[derive(Debug)]
pub enum DeckError {
    InvalidCod,
    EmptyName,
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for DeckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCod => formatter.write_str("Invalid Cockatrice deck"),
            Self::EmptyName => formatter.write_str("A saved deck needs a name"),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DeckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidCod | Self::EmptyName => None,
        }
    }
}

impl From<io::Error> for DeckError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DeckError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Reads a pasted deck list. The caller supplies the name when saving a text deck.
pub fn parse_text(text: &str) -> Deck {
    let mut deck = Deck::default();
    let mut sideboard = false;
    for original in text.trim_start_matches('\u{feff}').lines() {
        let line = original.trim();
        if line.is_empty() {
            sideboard |= !deck.main.is_empty();
            continue;
        }
        if line.trim_end_matches(':').eq_ignore_ascii_case("sideboard") {
            sideboard = true;
            continue;
        }
        let marked_side = line
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("SB:"));
        let entry = if marked_side {
            line[3..].trim_start()
        } else {
            line
        };
        if let Some(card) = text_entry(entry) {
            if sideboard || marked_side {
                deck.side.push(card);
            } else {
                deck.main.push(card);
            }
        } else {
            deck.skipped.push(original.to_owned());
        }
    }
    deck
}

fn text_entry(line: &str) -> Option<(String, u16)> {
    let (count, name) = line.split_once(char::is_whitespace)?;
    let count = count
        .strip_suffix(['x', 'X'])
        .unwrap_or(count)
        .parse()
        .ok()?;
    let name = without_printing(name.trim());
    (count != 0 && !name.is_empty()).then(|| (name.to_owned(), count))
}

fn without_printing(name: &str) -> &str {
    let Some((card, suffix)) = name.rsplit_once(" (") else {
        return name;
    };
    let Some((set, collector)) = suffix.split_once(')') else {
        return name;
    };
    let collector = collector.trim();
    if !set.is_empty()
        && set.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && collector.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && !collector.contains(char::is_whitespace)
    {
        card.trim_end()
    } else {
        name
    }
}

/// Distinct names absent from the store, with the first spelling and order from the list.
pub fn unknown(deck: &Deck, store: &Store<'_>) -> Vec<String> {
    let mut seen = HashSet::new();
    deck.main
        .iter()
        .chain(&deck.side)
        .filter(|(name, _)| {
            seen.insert(name.to_lowercase()) && store.card_ignoring_case(name).is_none()
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Counts a seat can share without revealing its decklist. Oversize totals saturate at `u16::MAX`.
pub fn counts(deck: &Deck) -> (u16, u16) {
    fn size(zone: &[(String, u16)]) -> u16 {
        zone.iter()
            .fold(0, |sum, (_, count)| sum.saturating_add(*count))
    }

    (size(&deck.main), size(&deck.side))
}
