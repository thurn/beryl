//! The card store's records for the names a client draws, shaped as the clients draw them
//! (`CardRecords`: `CardEntry` and `CardFace`, `ui/src/cards.ts`), with each face's image URLs
//! and the card back's expanded from the record's Scryfall ids.
//!
//! This crate links no engine and no card definitions, so that a client's tests and asset tools
//! can build records without them. Only one choice needs the cards: which printing a token name
//! that several printings share shows. The caller passes the cards that make each token as
//! [`TokenMakers`]; `beryl_session::card_records` passes the engine's.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use beryl_card_store::{Record, STORE, Store};
use beryl_protocol::card_records::{CardEntry, CardFace, CardImages, image};
pub use beryl_protocol::card_records::{CardRecords, CardRecordsRequest, PrintingIx};
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage};

/// `data/deck-printings.csv`, whose rows in order are the [`PrintingIx`] values.
const DECK_PRINTINGS: &str = include_str!("../../../../data/deck-printings.csv");

/// One art of a card that has several: a Secret Lair printing of a deck's card. An
/// [`ObjectView`](beryl_protocol::protocol::ObjectView)'s `printing` indexes [`PRINTINGS`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintingRecord {
    /// The deck key the printing belongs to, such as `forgetful-fish`.
    pub deck: &'static str,
    /// The card's name, which keys its ordinary record in the card store.
    pub name: &'static str,
    /// The printing's set code.
    pub set: &'static str,
    /// The printing's collector number within its set.
    pub collector_number: &'static str,
    /// How many of the deck's copies of the card show this printing.
    pub copies: u32,
    /// The printing's Scryfall id, from which its image addresses are expanded.
    pub scryfall_id: String,
}

/// The printing records, indexed by [`PrintingIx`]: one for each row of
/// `data/deck-printings.csv`, in the file's order, with the Scryfall id of the card store's
/// `printing:<set>/<collector_number>` record. Panics on first use if a row has no record, as
/// `just cards` keeps from happening.
pub static PRINTINGS: LazyLock<Vec<PrintingRecord>> = LazyLock::new(|| {
    DECK_PRINTINGS
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let [deck, name, set, collector_number, copies] =
                line.split(',').collect::<Vec<_>>()[..]
            else {
                panic!("deck-printings.csv row {line:?} does not have five columns");
            };
            let record = STORE.printing(set, collector_number).unwrap_or_else(|| {
                panic!("the card store has no printing {set}/{collector_number}")
            });
            PrintingRecord {
                deck,
                name,
                set,
                collector_number,
                copies: copies.parse().expect("copies is a number"),
                scryfall_id: record.scryfall_id,
            }
        })
        .collect()
});

/// The record of `ix`, if the table has one.
pub fn printing(ix: PrintingIx) -> Option<&'static PrintingRecord> {
    PRINTINGS.get(usize::try_from(ix.0).ok()?)
}

/// The printings of the card `name` in the deck `deck`, each with its [`PrintingIx`], in the
/// file's order. The copies of them sum to the deck's count of the card; none for a card
/// that has only its ordinary printing.
pub fn deck_printings(deck: &str, name: &str) -> Vec<(PrintingIx, &'static PrintingRecord)> {
    PRINTINGS
        .iter()
        .enumerate()
        .filter(|(_, p)| p.deck == deck && p.name == name)
        .map(|(i, p)| (PrintingIx(i as u32), p))
        .collect()
}

/// The entry a client draws for printing `ix`: its card's text with the printing's id, set,
/// collector number, artist and image addresses, expanded as a card's are.
pub fn printing_entry(ix: PrintingIx) -> Option<CardEntry> {
    let p = printing(ix)?;
    STORE.printing(p.set, p.collector_number).map(entry)
}

/// The cards that make each token name, as `(token name, card name)` pairs in the order the
/// cards are preferred. When several printings share a token's name, the one shown is the
/// printing linked by the first of these cards whose record links a token of that name, else
/// the first printing the store has.
#[derive(Clone, Copy, Debug, Default)]
pub struct TokenMakers<'a> {
    pairs: &'a [(&'a str, &'a str)],
}

impl<'a> TokenMakers<'a> {
    /// No cards: a shared token name shows the store's first printing.
    pub const NONE: TokenMakers<'static> = TokenMakers { pairs: &[] };

    /// The makers in `pairs`, each `(token name, card name)`, preferred first first.
    pub const fn new(pairs: &'a [(&'a str, &'a str)]) -> Self {
        TokenMakers { pairs }
    }

    /// The cards that make a token named `name`, preferred first first.
    fn of(self, name: &str) -> impl Iterator<Item = &'a str> + use<'a, '_> {
        self.pairs
            .iter()
            .filter(move |(token, _)| *token == name)
            .map(|&(_, card)| card)
    }
}

/// The committed store's records for `request`'s names.
pub fn card_records(request: &CardRecordsRequest, makers: TokenMakers) -> CardRecords {
    records_in(&STORE, &request.names, makers)
}

/// The committed store's records for every card or token name among `strings`, whatever else
/// they hold: a recorded fixture's, for every string its views and events show.
pub fn card_records_among<'a>(
    strings: impl IntoIterator<Item = &'a str>,
    makers: TokenMakers,
) -> CardRecords {
    let names: BTreeSet<&str> = strings.into_iter().collect();
    let names: Vec<String> = names.into_iter().map(str::to_owned).collect();
    records_in(&STORE, &names, makers)
}

/// A recorded fixture's `CARD_RECORDS` response: the records of every card or token name its
/// views and events show, so its page draws every card with no server.
pub fn fixture_message(messages: &[FixtureMessage], makers: TokenMakers) -> FixtureMessage {
    fn strings<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
        match value {
            serde_json::Value::String(s) => out.push(s),
            serde_json::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
            serde_json::Value::Object(map) => map.values().for_each(|v| strings(v, out)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for message in messages {
        if let FixtureMessage::Event { payload, .. } = message {
            strings(payload, &mut found);
        }
    }
    FixtureMessage::response(CARD_RECORDS, &card_records_among(found, makers))
}

/// Key the card-record lookup by exact token printing when names are shared.
pub fn token_printing_key(name: &str, scryfall_id: &str) -> String {
    format!("token:{name}:{scryfall_id}")
}

/// The same printing [`card_records`] gives for the supplied name.
pub fn record(name: &str, is_token: bool, makers: TokenMakers) -> Option<Record> {
    if is_token {
        token(&STORE, name, makers).or_else(|| STORE.card_ignoring_case(name))
    } else {
        STORE
            .card_ignoring_case(name)
            .or_else(|| token(&STORE, name, makers))
    }
}

fn records_in(store: &Store, names: &[String], makers: TokenMakers) -> CardRecords {
    let mut out = CardRecords::none();
    for name in names {
        if let Some((token_name, id)) = name
            .strip_prefix("token:")
            .and_then(|key| key.rsplit_once(':'))
            && let Some(printing) = store.token_printing(token_name, id)
        {
            out.tokens.insert(name.clone(), entry(printing));
            continue;
        }
        if let Some(card) = store.card_ignoring_case(name) {
            out.cards.insert(name.clone(), entry(card));
        }
        if let Some(token) = token(store, name, makers) {
            out.tokens.insert(name.clone(), entry(token));
        }
    }
    out
}

/// The store's printing of the token or emblem named `name`. Different tokens share a name (one
/// card's Insect is a white 2/1, another's a green 3/3), and each card's record links the one
/// it makes: so this is the one linked by the first of `makers` that makes a token of that
/// name, else the first printing the store has.
fn token(store: &Store, name: &str, makers: TokenMakers) -> Option<Record> {
    let mut printings = store.tokens(name);
    let linked = makers
        .of(name)
        .take_while(|_| printings.len() > 1)
        .filter_map(|card| store.card(card))
        .flat_map(|maker| maker.tokens)
        .find(|link| link.name == name);
    let at = linked
        .and_then(|link| {
            printings
                .iter()
                .position(|p| p.scryfall_id == link.scryfall_id)
        })
        .unwrap_or(0);
    (at < printings.len()).then(|| printings.swap_remove(at))
}

fn entry(record: Record) -> CardEntry {
    let id = record.scryfall_id;
    let faces = record
        .faces
        .into_iter()
        .map(|face| {
            let image = |version: &str| image(version, face.side.as_str(), &id);
            CardFace {
                images: CardImages {
                    art_crop: image("art_crop"),
                    normal: image("normal"),
                    large: image("large"),
                },
                name: face.name,
                mana_cost: face.mana_cost,
                type_line: face.type_line,
                oracle_text: face.oracle_text,
                power: face.power,
                toughness: face.toughness,
                loyalty: face.loyalty,
                colors: face.colors,
            }
        })
        .collect();
    CardEntry {
        name: record.name,
        scryfall_id: id,
        set: record.set,
        collector_number: record.collector_number,
        layout: record.layout,
        artist: record.artist,
        faces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECK: &str = "forgetful-fish";

    #[test]
    fn the_forgetful_fish_shows_two_dandans_and_four_islands_five_copies_each() {
        for (name, arts) in [("Dandân", 2), ("Island", 4)] {
            let found = deck_printings(DECK, name);
            assert_eq!(found.len(), arts, "{name}");
            assert!(found.iter().all(|(_, p)| p.set == "sld" && p.copies == 5));
        }
    }

    #[test]
    fn every_other_card_of_the_deck_has_one_printing_and_the_copies_make_eighty() {
        let all: Vec<_> = PRINTINGS.iter().filter(|p| p.deck == DECK).collect();
        assert_eq!(all.iter().map(|p| p.copies).sum::<u32>(), 80);
        let names: BTreeSet<_> = all.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), 23);
        for name in names
            .into_iter()
            .filter(|n| !["Dandân", "Island"].contains(n))
        {
            assert_eq!(deck_printings(DECK, name).len(), 1, "{name}");
        }
    }

    #[test]
    fn a_printing_s_entry_draws_its_own_art_beside_the_ordinary_card() {
        let islands = deck_printings(DECK, "Island");
        let ids: BTreeSet<_> = islands.iter().map(|(_, p)| p.scryfall_id.clone()).collect();
        assert_eq!(ids.len(), 4);
        let entry = printing_entry(islands[0].0).unwrap();
        assert_eq!(entry.name, "Island");
        assert_eq!(entry.scryfall_id, islands[0].1.scryfall_id);
        assert!(entry.faces[0].images.normal.contains(&entry.scryfall_id));
        let ordinary = record("Island", false, TokenMakers::NONE).unwrap();
        assert!(!ids.contains(&ordinary.scryfall_id));
    }

    #[test]
    fn an_index_past_the_table_has_no_printing() {
        assert!(printing(PrintingIx(PRINTINGS.len() as u32)).is_none());
    }
}
