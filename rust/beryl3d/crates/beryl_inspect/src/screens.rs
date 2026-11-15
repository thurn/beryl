//! Screen recordings: a client put on a screen, or a sheet opened over one, with no engine and
//! an empty view, as the stress boards are made (`crate::stress`). Each is named
//! `screen/<name>`, and `data/beryl3d/screens.json` lists them for the parity list.

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage};
use beryl_protocol::protocol::PlayerState;
use beryl_protocol::script::Reset;
use beryl_protocol::{GameUpdate, PlayerView, SessionId};
use beryl_scene::driver::OPEN;
use beryl_scene::testing;

/// What every screen recording's name starts with.
pub const PREFIX: &str = "screen/";

/// The name of every screen recording.
pub const NAMES: [&str; 6] = [
    "screen/start",
    "screen/table",
    "screen/gallery",
    "screen/testing",
    "screen/settings",
    "screen/bug-report",
];

/// The messages of the screen recording `name`, or none for any other name: the records of no
/// cards, the event that opens the screen or sheet (over the start screen), and one empty view.
#[must_use]
pub fn messages(name: &str) -> Option<Vec<FixtureMessage>> {
    if !NAMES.contains(&name) {
        return None;
    }
    let opened = name.strip_prefix(PREFIX)?;
    let opens = match opened {
        "settings" | "bug-report" => vec!["start", opened],
        _ => vec![opened],
    };
    let mut view = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    view.script = Some(testing::script(
        Some(Reset::default()),
        Vec::new(),
        Vec::new(),
    ));
    let mut messages = vec![FixtureMessage::response(CARD_RECORDS, &CardRecords::none())];
    messages.extend(opens.into_iter().map(|opened| FixtureMessage::Event {
        event: OPEN.to_owned(),
        payload: opened.into(),
    }));
    messages.push(FixtureMessage::game_update(&GameUpdate {
        session: SessionId(1),
        view,
    }));
    Some(messages)
}
