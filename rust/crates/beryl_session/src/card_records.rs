//! The [`crate::ipc::CARD_RECORDS`] command: the card store's records for the names the UI
//! draws, shaped as `ui/src/cards.ts` draws them (`CardEntry`, `CardFace`), with each face's
//! image URLs and the card back's expanded from the record's Scryfall ids. The UI asks for the
//! names a view shows; nothing is bundled into it or read at startup.

use std::collections::{BTreeMap, BTreeSet};

use beryl_card_store::{Record, STORE, Store};
use beryl_cards::CARDS;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::{CARD_RECORDS, FixtureMessage};

/// Scryfall's card image host: `<version>/<side>/<id[0]>/<id[1]>/<id>.jpg`.
const IMAGES: &str = "https://cards.scryfall.io";
/// Scryfall's card back host: `large/<id[0]>/<id[1]>/<id>.jpg`.
const BACKS: &str = "https://backs.scryfall.io/large";
/// Magic's card back, which Scryfall gives every card with a back of its own
/// (a record's `card_back_id`).
pub const CARD_BACK_ID: &str = "0aeebaf5-8c7d-4636-9e82-8c27447861f7";
/// The token whose art the UI gives a token with no name (a copy of a face-down permanent,
/// [CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a): "a 2/2 face-down creature
/// with no text, no name, no subtypes, and no mana cost"): Double Masters' Copy token.
pub const NAMELESS_TOKEN_ID: &str = "6870a2af-80e1-4813-a332-2b1bd789f1c1";

/// The [`crate::ipc::CARD_RECORDS`] command's request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CardRecordsRequest {
    /// Card, face, token and emblem names, as the engine gives them.
    pub names: Vec<String>,
}

/// The records found for a [`CardRecordsRequest`]'s names, each under the name asked for. A
/// name may be both a card's and a token's (a card and the offspring token it makes).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CardRecords {
    /// The image of Magic's card back.
    pub card_back: String,
    /// The art a token with no name is drawn with ([`NAMELESS_TOKEN_ID`]).
    pub nameless_token: String,
    /// The cards named so, or with a face so named.
    pub cards: BTreeMap<String, CardEntry>,
    /// The tokens and emblems named so: of each, the printing the card that makes it links.
    pub tokens: BTreeMap<String, CardEntry>,
}

/// A card's printing and faces, as the UI draws them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CardEntry {
    pub name: String,
    pub scryfall_id: String,
    pub set: String,
    pub collector_number: String,
    pub layout: String,
    pub artist: String,
    pub faces: Vec<CardFace>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CardFace {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mana_cost: Option<String>,
    pub type_line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub oracle_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub power: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub toughness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub loyalty: Option<String>,
    pub colors: Vec<String>,
    pub images: CardImages,
}

/// A face's image URLs on Scryfall, by version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CardImages {
    pub art_crop: String,
    pub normal: String,
    pub large: String,
}

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

fn records_in(store: &Store, names: &[String]) -> CardRecords {
    let mut out = CardRecords {
        card_back: back(CARD_BACK_ID),
        nameless_token: image("art_crop", "front", NAMELESS_TOKEN_ID),
        cards: BTreeMap::new(),
        tokens: BTreeMap::new(),
    };
    for name in names {
        if let Some(card) = store.card(name) {
            out.cards.insert(name.clone(), entry(card));
        }
        if let Some(token) = token(store, name) {
            out.tokens.insert(name.clone(), entry(token));
        }
    }
    out
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

/// Scryfall's image of the `side` of printing `id`, in `version`.
fn image(version: &str, side: &str, id: &str) -> String {
    format!(
        "{IMAGES}/{version}/{side}/{}/{}/{id}.jpg",
        &id[0..1],
        &id[1..2]
    )
}

fn back(id: &str) -> String {
    format!("{BACKS}/{}/{}/{id}.jpg", &id[0..1], &id[1..2])
}

#[cfg(test)]
mod tests;
