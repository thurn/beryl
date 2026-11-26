//! The records of the cards a view names, as the [`CARD_RECORDS`](crate::ipc::CARD_RECORDS)
//! command answers them: shaped as `ui/src/cards.ts` draws them (`CardEntry`, `CardFace`), with
//! each face's image URLs and the card back's expanded from the record's Scryfall ids.
//! `beryl_session::card_records` reads them from the card store.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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

/// An index into the printing records of `beryl_card_records` (`PRINTINGS`): one art of a card
/// that has several. It is defined here, and re-exported by `beryl_card_records`, because that
/// crate depends on this one and [`ObjectView`](crate::protocol::ObjectView) carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrintingIx(pub u32);

/// The [`CARD_RECORDS`](crate::ipc::CARD_RECORDS) command's request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardRecordsRequest {
    /// Card, face, token and emblem names, as the engine gives them.
    pub names: Vec<String>,
}

/// The records found for a [`CardRecordsRequest`]'s names, each under the name asked for. A
/// name may be both a card's and a token's (a card and the offspring token it makes).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardEntry {
    pub name: String,
    pub scryfall_id: String,
    pub set: String,
    pub collector_number: String,
    pub layout: String,
    pub artist: String,
    pub faces: Vec<CardFace>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardFace {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mana_cost: Option<String>,
    pub type_line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oracle_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toughness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loyalty: Option<String>,
    pub colors: Vec<String>,
    pub images: CardImages,
}

/// A face's image URLs on Scryfall, by version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardImages {
    pub art_crop: String,
    pub normal: String,
    pub large: String,
}

impl CardRecords {
    /// The records for names the card store does not have: the card back and the nameless
    /// token's art, and no card or token.
    #[must_use]
    pub fn none() -> CardRecords {
        CardRecords {
            card_back: back(CARD_BACK_ID),
            nameless_token: image("art_crop", "front", NAMELESS_TOKEN_ID),
            cards: BTreeMap::new(),
            tokens: BTreeMap::new(),
        }
    }
}

/// Scryfall's image of the `side` of printing `id`, in `version`.
pub fn image(version: &str, side: &str, id: &str) -> String {
    format!(
        "{IMAGES}/{version}/{side}/{}/{}/{id}.jpg",
        &id[0..1],
        &id[1..2]
    )
}

/// Scryfall's image of the card back `id`.
pub fn back(id: &str) -> String {
    format!("{BACKS}/{}/{}/{id}.jpg", &id[0..1], &id[1..2])
}
