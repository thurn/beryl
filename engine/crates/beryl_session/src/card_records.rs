//! The [`crate::ipc::CARD_RECORDS`] command: the card store's records for the names the UI
//! draws, shaped as `ui/src/cards.ts` draws them. The UI asks for the names a view shows;
//! nothing is bundled into it or read at startup.
//!
//! The records are built by `beryl_card_records`, which links no engine. This module adds the
//! engine's cards as the [`TokenMakers`] that choose among a shared token name's printings.

use std::sync::OnceLock;

pub use beryl_card_records::{TokenMakers, add_printings, token_printing_key};
use beryl_card_store::Record;
use beryl_engine::CardTable;
pub use beryl_protocol::card_records::*;

use crate::ipc::FixtureMessage;

/// Each token or emblem name an implemented card makes, with the name of the card's first face,
/// in the order of the card table: a shared token name shows the printing that the first card the
/// engine implements that makes it links.
static ENGINE_MAKERS: OnceLock<(&'static CardTable, Vec<(&'static str, &'static str)>)> =
    OnceLock::new();

/// The pairs for each token the cards of `cards` make.
fn makers_of(cards: &'static CardTable) -> Vec<(&'static str, &'static str)> {
    let mut pairs = Vec::new();
    for card in cards.cards() {
        let Some(maker) = card.face_ixs().next().map(|f| card.face_name(f)) else {
            continue;
        };
        for token in card.tokens {
            pairs.extend(token.face_ixs().map(|f| (token.face_name(f), maker)));
        }
    }
    pairs
}

/// The cards of `cards` as the makers of the tokens they make. The makers are computed once
/// and kept, so every call of a process passes the same table.
pub fn engine_makers(cards: &'static CardTable) -> TokenMakers<'static> {
    let (kept, pairs) = ENGINE_MAKERS.get_or_init(|| (cards, makers_of(cards)));
    debug_assert!(
        std::ptr::eq(*kept, cards),
        "a process uses one card table for its token makers"
    );
    TokenMakers::new(pairs)
}

/// The committed store's records for `request`'s names.
pub fn card_records(request: &CardRecordsRequest, cards: &'static CardTable) -> CardRecords {
    beryl_card_records::card_records(request, engine_makers(cards))
}

/// The committed store's records for every card or token name among `strings`, whatever else
/// they hold: a recorded fixture's, for every string its views and events show.
pub fn card_records_among<'a>(
    strings: impl IntoIterator<Item = &'a str>,
    cards: &'static CardTable,
) -> CardRecords {
    beryl_card_records::card_records_among(strings, engine_makers(cards))
}

/// A recorded fixture's [`crate::ipc::CARD_RECORDS`] response: the records of every card or
/// token name its views and events show, so its page draws every card with no server.
pub fn fixture_message(messages: &[FixtureMessage], cards: &'static CardTable) -> FixtureMessage {
    beryl_card_records::fixture_message(messages, engine_makers(cards))
}

/// The same printing the card records endpoint uses for the supplied name.
pub fn record(name: &str, is_token: bool, cards: &'static CardTable) -> Option<Record> {
    beryl_card_records::record(name, is_token, engine_makers(cards))
}
