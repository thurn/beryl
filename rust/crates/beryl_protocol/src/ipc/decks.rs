//! Requests and replies for the deck library and card store.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeckImportRequest {
    pub name: String,
    #[ts(optional)]
    pub text: Option<String>,
    #[ts(optional)]
    pub cod: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeckImportReply {
    pub name: String,
    pub main: Vec<(String, u16)>,
    pub side: Vec<(String, u16)>,
    pub unknown: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedDeck {
    pub deck: DeckImportReply,
    pub main_count: u16,
    pub side_count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeckDeleteRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DeckOpenReply {
    Text { text: String },
    Cod { text: String },
    Canceled,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CardSearchRequest {
    pub text: String,
    pub tokens: bool,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CardSearchHit {
    pub name: String,
    pub scryfall_id: String,
    pub type_line: String,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mana_cost: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toughness: Option<String>,
    pub image: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CardTokensRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CardTokenLink {
    pub name: String,
    pub scryfall_id: String,
}

pub fn typescript_decls(cfg: &ts_rs::Config) -> [String; 9] {
    [
        DeckImportRequest::decl(cfg),
        DeckImportReply::decl(cfg),
        SavedDeck::decl(cfg),
        DeckDeleteRequest::decl(cfg),
        DeckOpenReply::decl(cfg),
        CardSearchRequest::decl(cfg),
        CardSearchHit::decl(cfg),
        CardTokensRequest::decl(cfg),
        CardTokenLink::decl(cfg),
    ]
}
