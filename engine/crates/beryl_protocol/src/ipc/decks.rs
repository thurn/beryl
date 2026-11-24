//! Requests and replies for the deck library and card store.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckImportRequest {
    pub name: String,
    pub text: Option<String>,
    pub cod: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckImportReply {
    pub name: String,
    pub main: Vec<(String, u16)>,
    pub side: Vec<(String, u16)>,
    pub unknown: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDeck {
    pub deck: DeckImportReply,
    pub main_count: u16,
    pub side_count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckDeleteRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DeckOpenReply {
    Text { text: String },
    Cod { text: String },
    Canceled,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardSearchRequest {
    pub text: String,
    pub tokens: bool,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSearchHit {
    pub name: String,
    pub scryfall_id: String,
    pub type_line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mana_cost: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toughness: Option<String>,
    pub image: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardTokensRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardTokenLink {
    pub name: String,
    pub scryfall_id: String,
}
