//! The card store's records for the names a client draws, shaped as the clients draw them
//! (`CardRecords`: `CardEntry` and `CardFace`, `ui/src/cards.ts`), with each face's image URLs
//! and the card back's expanded from the record's Scryfall ids.
//!
//! This crate links no engine and no card definitions, so that a client's tests and asset tools
//! can build records without them. Only one choice needs the cards: which printing a token name
//! that several printings share shows. The caller passes the cards that make each token as
//! [`TokenMakers`]; `beryl_session::card_records` passes the engine's.

use std::collections::BTreeSet;

use beryl_card_store::{Record, STORE, Store};
use beryl_protocol::card_records::{CardEntry, CardFace, CardImages, image};
pub use beryl_protocol::card_records::{CardRecords, CardRecordsRequest};
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage};

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
