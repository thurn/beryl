//! The [`crate::ipc::CARD_RECORDS`] command: the card store's records for the names the UI
//! draws, shaped as `ui/src/cards.ts` draws them. The UI asks for the names a view shows;
//! nothing is bundled into it or read at startup.
//!
//! The records are built by `beryl_card_records`, which links no engine. This module adds the
//! engine's cards as the [`TokenMakers`] that choose among a shared token name's printings.

use std::sync::LazyLock;

pub use beryl_card_records::{TokenMakers, token_printing_key};
use beryl_card_store::Record;
use beryl_cards::CARDS;
pub use beryl_protocol::card_records::*;

use crate::ipc::FixtureMessage;

/// Each token or emblem name an implemented card makes, with the name of the card's first face,
/// in the order of [`CARDS`]: a shared token name shows the printing that the first card the
/// engine implements that makes it links.
static ENGINE_MAKERS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    let mut pairs = Vec::new();
    for card in CARDS.cards() {
        let Some(maker) = card.face_ixs().next().map(|f| card.face_name(f)) else {
            continue;
        };
        for token in card.tokens {
            pairs.extend(token.face_ixs().map(|f| (token.face_name(f), maker)));
        }
    }
    pairs
});

/// The engine's cards as the makers of the tokens they make.
pub fn engine_makers() -> TokenMakers<'static> {
    TokenMakers::new(&ENGINE_MAKERS)
}

/// The committed store's records for `request`'s names.
pub fn card_records(request: &CardRecordsRequest) -> CardRecords {
    beryl_card_records::card_records(request, engine_makers())
}

/// The committed store's records for every card or token name among `strings`, whatever else
/// they hold: a recorded fixture's, for every string its views and events show.
pub fn card_records_among<'a>(strings: impl IntoIterator<Item = &'a str>) -> CardRecords {
    beryl_card_records::card_records_among(strings, engine_makers())
}

/// A recorded fixture's [`crate::ipc::CARD_RECORDS`] response: the records of every card or
/// token name its views and events show, so its page draws every card with no server.
pub fn fixture_message(messages: &[FixtureMessage]) -> FixtureMessage {
    beryl_card_records::fixture_message(messages, engine_makers())
}

/// The same printing the card records endpoint uses for the supplied name.
pub fn record(name: &str, is_token: bool) -> Option<Record> {
    beryl_card_records::record(name, is_token, engine_makers())
}

#[cfg(test)]
mod tests;
