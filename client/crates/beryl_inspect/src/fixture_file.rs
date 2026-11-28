//! A recording read from a fixture file on disk rather than from the pack: a bug report's
//! `fixture.json`, which holds the latest views the client was given before the report was
//! saved (`beryl_session::bugs`). Every command that reads a recording takes the file's path in
//! place of a recording's name, so `just client bug` can draw the reported moment with `png`,
//! `film`, `tree` and `story from`.
//!
//! A report's fixture holds no card records, because a live game answers the client's
//! `card_records` request from the session. The file is given the committed card store's
//! records for every card its views name, as the recorder gives each recording in the pack.

use std::path::Path;

use app_client::driver::Recording;
use beryl_card_records::TokenMakers;
use beryl_protocol::ipc::{CARD_RECORDS, Fixture, FixtureMessage};
use beryl_protocol::protocol::Side;

/// Whether `name` is the path of a fixture file: a `.json` file that exists.
#[must_use]
pub fn is_fixture_file(name: &str) -> bool {
    let path = Path::new(name);
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        && path.is_file()
}

/// The recording in the fixture file at `path`, as `seat` sees it, with card records added
/// when it has none.
///
/// # Errors
///
/// When `seat` is the opponent, because a report's views are its own player's, or when the
/// file cannot be read as a fixture: `{"messages": [...]}` or a list of messages, as
/// `just client export` writes them.
pub fn read(path: &str, seat: Side) -> Result<Recording, String> {
    if seat != Side::You {
        return Err(format!("{path} holds one player's views; use --seat you"));
    }
    let why = |error: &dyn std::fmt::Display| format!("{path}: {error}");
    let text = std::fs::read_to_string(path).map_err(|error| why(&error))?;
    let mut messages = match serde_json::from_str::<Fixture>(&text) {
        Ok(fixture) => fixture.messages,
        Err(_) => serde_json::from_str::<Vec<FixtureMessage>>(&text).map_err(|error| {
            why(&format!(
                "not a fixture, {{\"messages\": [...]}} or a list of messages: {error}"
            ))
        })?,
    };
    let has_cards = messages.iter().any(|message| {
        matches!(message, FixtureMessage::Response { command, .. } if command == CARD_RECORDS)
    });
    if !has_cards {
        let records = beryl_card_records::fixture_message(&messages, TokenMakers::NONE);
        messages.insert(0, records);
    }
    Ok(Recording(messages))
}
