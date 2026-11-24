use beryl_session::ipc::FixtureMessage;
use beryl_session::protocol::PlayerView;
use beryl_session::{GameUpdate, Status};
use std::path::Path;

use crate::common::TEST;
use beryl_session::protocol::Kind;

use beryl_cli::fixture::*;

#[test]
fn a_recording_stops_at_its_condition() {
    // Fixed decks: fuzz decks are drawn from every card, so a new card changes the game.
    let def: Definition = serde_json::from_value(serde_json::json!({
        "name": "t",
        "seed": 4,
        "decks": ["wu-control", "wu-control"],
        "agents": ["random", "random"],
        "until": "first target decision for seat 1",
    }))
    .unwrap();
    let messages = record_game(&TEST, &def, false).unwrap();
    let Some(FixtureMessage::Event { payload, .. }) = messages.last() else {
        panic!("a recording ends on a view");
    };
    let update: GameUpdate = serde_json::from_value(payload.clone()).unwrap();
    let Status::Deciding { decision } = update.view.status else {
        panic!("a recording stops at a decision");
    };
    assert_eq!(decision.kind, Kind::Target);
}

/// The views of `messages`, a recording.
fn views(messages: &[FixtureMessage]) -> Vec<PlayerView> {
    (messages.iter())
        .filter_map(|message| match message {
            FixtureMessage::Event { payload, .. } => Some(
                serde_json::from_value::<GameUpdate>(payload.clone())
                    .unwrap()
                    .view,
            ),
            FixtureMessage::Response { .. } => None,
        })
        .collect()
}

/// Seen from across the table, a recording is the same game to the same stop, a view of
/// each update the human was shown, from the other side and with nothing to decide: for a
/// seeded game, and for a board whose clicks play on past its stop.
#[test]
fn a_recording_from_across_the_table_shows_the_same_updates() {
    let seeded = serde_json::json!({
        "name": "t",
        "seed": 4,
        "decks": ["wu-control", "wu-control"],
        "agents": ["random", "random"],
        "until": "first target decision for seat 1",
    });
    let clicked = serde_json::json!({
        "name": "t",
        "board": "shape-pick",
        "until": "first priority decision for seat 0",
        "clicks": ["Counterspell", "Island", "press decline"],
    });
    for json in [seeded, clicked] {
        let def: Definition = serde_json::from_value(json).unwrap();
        let seats = def.seats().unwrap();
        assert_eq!(seats.len(), 2);
        let path = Path::new("unused");
        let human = views(&record_from(&TEST, &def, path, seats[0]).unwrap());
        let across = views(&record_from(&TEST, &def, path, seats[1]).unwrap());
        assert_eq!(human.len(), across.len(), "{}", def.name);
        for (mine, theirs) in human.iter().zip(&across) {
            assert_eq!(mine.turn.number, theirs.turn.number);
            assert_eq!(mine.turn.step, theirs.turn.step);
            assert_ne!(mine.turn.active, theirs.turn.active);
            assert!(!matches!(theirs.status, Status::Deciding { .. }));
        }
    }
}
