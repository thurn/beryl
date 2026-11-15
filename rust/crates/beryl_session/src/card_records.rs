//! The [`crate::ipc::CARD_RECORDS`] command: the card store's records for the names the UI
//! draws, shaped as `ui/src/cards.ts` draws them (`CardEntry`, `CardFace`), with each face's
//! image URLs and the card back's expanded from the record's Scryfall ids. The UI asks for the
//! names a view shows; nothing is bundled into it or read at startup.

use std::collections::BTreeSet;

use beryl_card_store::{Record, STORE, Store};
use beryl_cards::CARDS;
pub use beryl_protocol::card_records::*;

use crate::ipc::{CARD_RECORDS, FixtureMessage};

/// The committed store's records for `request`'s names.
pub fn card_records(request: &CardRecordsRequest) -> CardRecords {
    records_in(&STORE, &request.names)
}

/// The committed store's records for every card or token name among `strings`, whatever else
/// they hold: a recorded fixture's, for every string its views and events show.
pub fn card_records_among<'a>(strings: impl IntoIterator<Item = &'a str>) -> CardRecords {
    let names: BTreeSet<&str> = strings.into_iter().collect();
    let names: Vec<String> = names.into_iter().map(str::to_owned).collect();
    records_in(&STORE, &names)
}

/// A recorded fixture's [`crate::ipc::CARD_RECORDS`] response: the records of every card or
/// token name its views and events show, so its page draws every card with no server.
pub fn fixture_message(messages: &[FixtureMessage]) -> FixtureMessage {
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
    FixtureMessage::response(CARD_RECORDS, &card_records_among(found))
}

/// Key the card-record lookup by exact token printing when names are shared.
pub fn token_printing_key(name: &str, scryfall_id: &str) -> String {
    format!("token:{name}:{scryfall_id}")
}

fn records_in(store: &Store, names: &[String]) -> CardRecords {
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
        if let Some(token) = token(store, name) {
            out.tokens.insert(name.clone(), entry(token));
        }
    }
    out
}

/// The same printing the card records endpoint uses for the supplied name.
pub fn record(name: &str, is_token: bool) -> Option<Record> {
    if is_token {
        token(&STORE, name).or_else(|| STORE.card_ignoring_case(name))
    } else {
        STORE
            .card_ignoring_case(name)
            .or_else(|| token(&STORE, name))
    }
}

/// The store's printing of the token or emblem named `name`. Different tokens share a name (one
/// card's Insect is a white 2/1, another's a green 3/3), and each card's record links the one
/// it makes: so this is the one linked by the first card the engine implements that makes a
/// token of that name, else the first printing the store has.
fn token(store: &Store, name: &str) -> Option<Record> {
    let mut printings = store.tokens(name);
    let linked = CARDS
        .cards()
        .iter()
        .filter(|card| {
            let mut made = card.tokens.iter();
            printings.len() > 1 && made.any(|t| t.face_ixs().any(|f| t.face_name(f) == name))
        })
        .filter_map(|card| store.card(card.face_name(card.face_ixs().next()?)))
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
mod tests;
