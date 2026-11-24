use super::*;
use beryl_table::log::{HistoryLine, UndoControl};

fn row(game: &Game, viewer: Seat, n: u32) -> HistoryLine<'_> {
    game.log
        .history(&game.table, viewer)
        .find(|line| line.n == n)
        .unwrap()
}

fn assert_undo(game: &Game, by: Seat, entry: Option<u32>, expected: Result<u32, Refusal>) {
    let query = game.log.undo_target(&game.table, by, entry);
    let (mut table, mut log, mut chance) = save::open_with_chance(&game.file[..]).unwrap();
    let applied = log
        .apply(&mut table, by, &Action::Undo { entry }, &mut chance)
        .map(|entry| entry.undoes.unwrap());
    assert_eq!([query, applied], [expected.clone(), expected]);
}

#[test]
fn a_draw_stays_available_to_its_actor_after_a_tap() {
    let mut game = Game::new(fixtures());
    let drawn = game.act(HOST, &draw(1)).n;
    let tapped = game.act(HOST, &tap(1, true)).n;
    assert_eq!(game.log.last_draw(&game.table, HOST), Some(drawn));
    assert_eq!(game.log.last_draw(&game.table, GUEST), None);
    assert_eq!(row(&game, HOST, drawn).undo, None);
    assert_undo(&game, HOST, None, Ok(tapped));
    assert_undo(&game, HOST, Some(drawn), Ok(drawn));
    assert_undo(&game, GUEST, Some(drawn), Err(Refusal::INVALID));
    assert_undo(&game, GUEST, None, Err(Refusal::INVALID));

    let undo = game.act(HOST, &Action::Undo { entry: Some(drawn) }).n;
    assert_eq!(game.log.last_draw(&game.table, HOST), None);
    assert!(game.table.objects[&ObjectId(1)].tapped);
    for viewer in [HOST, GUEST] {
        assert_eq!(row(&game, viewer, undo).text, "Undid: Drew 1 card (seen).");
        assert_undo(&game, viewer, Some(drawn), Err(Refusal::INVALID));
    }
    game.assert_reopens();
}

#[test]
fn a_blocked_draw_does_not_fall_back_to_an_older_draw() {
    let mut game = Game::new(fixtures());
    game.act(HOST, &draw(1));
    let drawn = game.act(HOST, &draw(1)).n;
    let id = game.table.zone(HAND).unwrap()[0];
    let moved = game
        .act(HOST, &move_card(id.0, HAND, BATTLEFIELD, Position::Top))
        .n;
    assert_eq!(game.log.last_draw(&game.table, HOST), Some(drawn));
    assert_undo(
        &game,
        HOST,
        Some(drawn),
        Err(Refusal::Invalid {
            blocking: vec![moved],
        }),
    );
}

#[test]
fn both_seats_see_the_live_blockers_and_rewind_updates_batch_controls() {
    let mut game = Game::new(fixtures());
    let batch = game
        .act(
            HOST,
            &Action::Batch {
                actions: vec![tap(1, true)],
            },
        )
        .n;
    let first = game.act(GUEST, &tap(1, false)).n;
    game.act(HOST, &life(HOST, 2));
    let second = game.act(GUEST, &tap(1, true)).n;
    let blockers = vec![first, second];
    for viewer in [HOST, GUEST] {
        assert_eq!(
            row(&game, viewer, batch).undo,
            Some(UndoControl::Blocked(blockers.clone()))
        );
        assert_undo(
            &game,
            viewer,
            Some(batch),
            Err(Refusal::Invalid {
                blocking: blockers.clone(),
            }),
        );
    }

    let rewind = game.act(GUEST, &Action::Rewind { to: batch }).n;
    for viewer in [HOST, GUEST] {
        assert!(row(&game, viewer, first).struck);
        assert!(row(&game, viewer, second).struck);
        assert_eq!(
            row(&game, viewer, batch).undo,
            Some(UndoControl::Blocked(vec![rewind]))
        );
        assert_undo(
            &game,
            viewer,
            Some(batch),
            Err(Refusal::Invalid {
                blocking: vec![rewind],
            }),
        );
    }
    assert_undo(&game, GUEST, None, Ok(rewind));
    game.act(GUEST, &UNDO);
    assert!(!row(&game, GUEST, second).struck);
    assert_eq!(
        row(&game, GUEST, batch).undo,
        Some(UndoControl::Blocked(blockers))
    );

    game.act(GUEST, &UNDO);
    game.act(GUEST, &UNDO);
    for viewer in [HOST, GUEST] {
        assert_eq!(row(&game, viewer, batch).undo, Some(UndoControl::Available));
        assert_undo(&game, viewer, Some(batch), Ok(batch));
    }
    game.act(GUEST, &Action::Undo { entry: Some(batch) });
    assert_eq!(row(&game, HOST, batch).undo, None);
    game.act(HOST, &Action::Rewind { to: 0 });
    assert!(row(&game, GUEST, batch).struck);
    assert_eq!(row(&game, GUEST, batch).undo, None);
    game.assert_reopens();
}

#[test]
fn recorded_wording_survives_an_id_change_and_save_replay() {
    let mut initial = fixtures();
    let secret = "Private card name";
    add(&mut initial, 5, secret, BATTLEFIELD, &[HOST]);
    let mut game = Game::new(initial);
    let labeled = game
        .act(
            HOST,
            &Action::Counter {
                selection: Selection {
                    zone: BATTLEFIELD,
                    objects: vec![ObjectId(5)],
                },
                name: secret.into(),
                adjustment: Adjustment::Add(1),
            },
        )
        .n;
    let moved = game
        .act(HOST, &move_card(5, BATTLEFIELD, HAND, Position::Top))
        .n;
    assert!(
        game.table
            .zone(HAND)
            .unwrap()
            .iter()
            .all(|id| *id != ObjectId(5))
    );
    assert_eq!(
        row(&game, HOST, moved).text,
        "Put Private card name from the battlefield into your hand."
    );
    assert_eq!(
        row(&game, GUEST, moved).text,
        "Put a card from the battlefield into Seat 1's hand."
    );
    assert_eq!(
        row(&game, HOST, labeled).text,
        "Added 1 Private card name counters on Private card name."
    );
    assert_eq!(
        row(&game, GUEST, labeled).text,
        "Added 1 a counters on a card."
    );

    let hand_id = game.table.zone(HAND).unwrap()[0];
    game.act(
        HOST,
        &move_card(hand_id.0, HAND, BATTLEFIELD, Position::Top),
    );
    assert_eq!(game.table.objects[&hand_id].known_to, [HOST, GUEST]);
    assert_eq!(
        row(&game, GUEST, labeled).text,
        "Added 1 a counters on a card."
    );
    game.act(HOST, &UNDO);
    let undo = game.act(HOST, &UNDO).n;
    for viewer in [HOST, GUEST] {
        assert_eq!(
            row(&game, viewer, undo).text,
            format!("Undid: {}", row(&game, viewer, moved).text)
        );
    }
    assert_eq!(row(&game, GUEST, undo).by, HOST);
    assert_eq!(
        row(&game, GUEST, labeled).text,
        "Added 1 a counters on a card."
    );
    game.assert_reopens();
}

#[test]
fn moving_to_a_private_hand_preserves_the_earlier_public_line() {
    let mut game = Game::new(fixtures());
    let tapped = game.act(HOST, &tap(1, true)).n;
    let moved = game
        .act(HOST, &move_card(1, BATTLEFIELD, HAND, Position::Top))
        .n;
    assert!(!game.table.objects.contains_key(&ObjectId(1)));
    assert_eq!(row(&game, GUEST, tapped).text, "Tapped Underground Sea.");
    let published = |viewer| {
        game.log
            .visible_history(&game.table, viewer)
            .find(|row| row.n == tapped)
            .unwrap()
            .text
    };
    assert_eq!(published(HOST), "Tapped Underground Sea.");
    assert_eq!(published(GUEST), "Tapped a card.");
    assert_eq!(
        row(&game, GUEST, moved).text,
        "Put a card from the battlefield into Seat 1's hand."
    );
    game.assert_reopens();
}
