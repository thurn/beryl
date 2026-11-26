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

#[test]
fn deck_names_map_to_constructed_and_shared_decks() {
    let (format, decks) = start_decks(&TEST, &["wu-control".into(), "wu-control".into()]).unwrap();
    assert_eq!(format, beryl_session::Format::CONSTRUCTED);
    assert!(!decks[0].is_empty() && !decks[1].is_empty());

    let (format, decks) = start_decks(&TEST, &["fuzz:7".into(), "fuzz:8".into()]).unwrap();
    assert_eq!(format, beryl_session::Format::CONSTRUCTED);
    assert_eq!(
        decks[0].iter().map(|(n, _)| usize::from(*n)).sum::<usize>(),
        40
    );
    assert_eq!(
        decks[1].iter().map(|(n, _)| usize::from(*n)).sum::<usize>(),
        40
    );

    let (format, decks) =
        start_decks(&TEST, &["forgetful-fish".into(), "forgetful-fish".into()]).unwrap();
    assert_eq!(format, beryl_session::Format::SHARED_DECK);
    assert_eq!(
        decks[0].iter().map(|(n, _)| usize::from(*n)).sum::<usize>(),
        80
    );
    assert!(decks[1].is_empty());

    let (format, decks) =
        start_decks(&TEST, &["shared-fuzz:7".into(), "shared-fuzz:7".into()]).unwrap();
    assert_eq!(format, beryl_session::Format::SHARED_DECK);
    assert_eq!(
        decks[0].iter().map(|(n, _)| usize::from(*n)).sum::<usize>(),
        80
    );
    assert!(decks[1].is_empty());

    assert!(start_decks(&TEST, &["forgetful-fish".into(), "wu-control".into()]).is_err());
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
