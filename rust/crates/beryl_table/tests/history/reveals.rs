use std::io::Write;

use beryl_table::action::{Portion, Position, Selection};
use beryl_table::permissions::{check, reference_zone};
use beryl_table::save;
use beryl_table::state::{ObjectId, Zone};
use beryl_table::{Action, Refusal, Table};

use super::{BATTLEFIELD, GUEST, Game, HAND, HOST, LIBRARY, UNDO, add, draw, fixtures, move_card};

#[test]
fn reveal_grants_replay_and_undo_independently_of_retained_knowledge() {
    let mut game = Game::new(fixtures());
    let card = ObjectId(2);
    let reveal = Action::Reveal {
        zone: LIBRARY,
        objects: Some(vec![card]),
        random: None,
        to: vec![GUEST],
    };
    game.act(HOST, &reveal);
    game.act(HOST, &UNDO);
    assert!(game.table.objects[&card].known_to.contains(&GUEST));
    assert_eq!(reference_zone(&game.table, GUEST, card), None);
    game.assert_reopens();

    game.act(HOST, &reveal);
    assert_eq!(reference_zone(&game.table, GUEST, card), Some(LIBRARY));
    game.act(
        HOST,
        &Action::Shuffle {
            seat: HOST,
            portion: Portion::Top(1),
        },
    );
    let fresh = game.table.zone(LIBRARY).unwrap()[0];
    assert_ne!(fresh, card);
    assert_eq!(reference_zone(&game.table, GUEST, fresh), None);
    assert_eq!(
        check(
            &game.table,
            GUEST,
            &move_card(fresh.0, LIBRARY, BATTLEFIELD, Position::Top)
        ),
        Err(Refusal::Permission)
    );
    game.assert_reopens();

    game.act(HOST, &UNDO);
    assert_eq!(reference_zone(&game.table, GUEST, card), Some(LIBRARY));
    game.act(HOST, &draw(1));
    let drawn = game.table.zone(HAND).unwrap()[0];
    assert_ne!(drawn, card);
    assert_eq!(reference_zone(&game.table, GUEST, drawn), None);
    assert_eq!(
        check(
            &game.table,
            GUEST,
            &move_card(drawn.0, HAND, BATTLEFIELD, Position::Top)
        ),
        Err(Refusal::Permission)
    );
    game.assert_reopens();

    game.act(HOST, &UNDO);
    assert_eq!(reference_zone(&game.table, GUEST, card), Some(LIBRARY));
    game.act(GUEST, &move_card(2, LIBRARY, BATTLEFIELD, Position::Top));
    game.act(GUEST, &move_card(2, BATTLEFIELD, LIBRARY, Position::Top));
    assert!(game.table.objects[&card].known_to.contains(&GUEST));
    assert_eq!(reference_zone(&game.table, GUEST, card), None);
    game.assert_reopens();
}

#[test]
fn legacy_hand_actions_and_undos_replay_before_explicit_reveals_take_effect() {
    let mut initial = Table::new(2);
    add(&mut initial, 1, "Island", HAND, &[HOST]);
    let card = ObjectId(1);
    let reveal = Action::Reveal {
        zone: HAND,
        objects: Some(vec![card]),
        random: None,
        to: vec![GUEST],
    };
    let note = Action::Note {
        selection: Selection {
            objects: vec![card],
            zone: HAND,
        },
        note: Some("hand note".into()),
    };
    let discard = move_card(1, HAND, Zone::Graveyard(HOST), Position::Top);
    let mut file = Vec::new();
    save::write_header(&mut file, &initial, 17).unwrap();
    // These entries use the original save schema and hand permissions.
    for (index, (by, action)) in [
        (HOST, &reveal),
        (HOST, &UNDO),
        (GUEST, &note),
        (HOST, &reveal),
        (GUEST, &UNDO),
        (GUEST, &discard),
    ]
    .into_iter()
    .enumerate()
    {
        let entry = serde_json::json!({
            "n": index + 1,
            "by": by,
            "action": action,
            "outcomes": [],
        });
        writeln!(file, "{entry}").unwrap();
    }

    let (table, log, chance) = save::open_with_chance(&file[..]).unwrap();
    assert_eq!(table.zone(Zone::Graveyard(HOST)).unwrap(), [card]);
    assert_eq!(table.objects[&card].note, None);
    let mut game = Game {
        table,
        log,
        chance,
        file,
    };
    // Rebuilding an older moment must accept its original hand edits too.
    game.act(HOST, &Action::Rewind { to: 3 });
    assert_eq!(game.table.objects[&card].note.as_deref(), Some("hand note"));
    assert_eq!(reference_zone(&game.table, GUEST, card), None);
    assert_eq!(
        game.log
            .apply(&mut game.table, GUEST, &discard, &mut game.chance),
        Err(Refusal::Permission)
    );
    game.act(HOST, &reveal);
    game.act(GUEST, &discard);
    game.assert_reopens();
}
