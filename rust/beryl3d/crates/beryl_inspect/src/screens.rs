//! Screen recordings: a client put on a screen, or a sheet opened over one, with no engine and
//! an empty view, as the stress boards are made (`crate::stress`). Each is named
//! `screen/<name>`, and `data/beryl3d/screens.json` lists them for the parity list. The manual
//! table's is a view of a solo table built by hand, with a few cards in each zone, which opens
//! the table screen itself.

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage, LIST_AGENTS};
use beryl_protocol::protocol::PlayerState;
use beryl_protocol::script::Reset;
use beryl_protocol::{AgentView, GameUpdate, PlayerView, SessionId};
use beryl_scene::driver::{OPEN, RECORDINGS};
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

/// The agents the screen recordings offer: three of the host's, one of which cheats, so that the
/// start screen shows each kind of row.
fn agents() -> Vec<AgentView> {
    [
        (
            "mcts-omniscient-quick",
            "Quick Seer",
            "Cheats as the Seer does, answering in a moment",
        ),
        (
            "mcts-hidden",
            "Searcher",
            "Searches many guesses at the cards it cannot see, playing each out greedily",
        ),
        (
            "expert",
            "Expert",
            "Plays by a strong player's rules for this deck, and works out combat",
        ),
    ]
    .map(|(name, title, description)| AgentView {
        name: name.to_owned(),
        title: title.to_owned(),
        description: description.to_owned(),
    })
    .to_vec()
}

/// The messages of the screen recording `name`, or none for any other name: the records of no
/// cards, the agents a person may play, the event that opens the screen or sheet (over the
/// start screen), and one view: empty, or the hand-built table's.
#[must_use]
pub fn messages(name: &str) -> Option<Vec<FixtureMessage>> {
    if !NAMES.contains(&name) {
        return None;
    }
    let opened = name.strip_prefix(PREFIX)?;
    let opens = match opened {
        "settings" | "bug-report" => vec!["start", opened],
        // A table's view opens the table screen; the start screen's press would start a table.
        "table" => Vec::new(),
        _ => vec![opened],
    };
    let mut view = if opened == "table" {
        let mut view = testing::ViewBuilder::sample().build();
        view.table = Some(testing::table(&view));
        view
    } else {
        PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20))
    };
    view.script = Some(testing::script(
        Some(Reset::default()),
        Vec::new(),
        Vec::new(),
    ));
    // The table's cards have placeholder faces, as a stress board's do.
    let records = if opened == "table" {
        crate::stress::placeholder_records(std::slice::from_ref(&view))
    } else {
        CardRecords::none()
    };
    let mut messages = vec![
        FixtureMessage::response(CARD_RECORDS, &records),
        FixtureMessage::response(LIST_AGENTS, &agents()),
    ];
    if opened == "testing" {
        messages.push(FixtureMessage::Event {
            event: RECORDINGS.to_owned(),
            payload: crate::recordings::listed().unwrap_or_default().into(),
        });
    }
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
