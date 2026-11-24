use beryl_table::state::{AbilityMarker, Face};

use super::*;

fn create_public_token(game: &mut Game) -> (ObjectId, Card) {
    let token = Card::Custom {
        name: "Public Token".into(),
        types: "Artifact".into(),
        colors: Vec::new(),
        stats: None,
    };
    game.act(
        HOST,
        &Action::Token {
            card: token.clone(),
            count: 1,
            attached_to: None,
            note: None,
            face_down: false,
        },
    );
    (game.table.battlefield[0], token)
}

#[test]
fn replay_keeps_each_ability_source_after_it_leaves_the_battlefield() {
    let mut table = Table::new(2);
    add(
        &mut table,
        1,
        "Public Permanent",
        BATTLEFIELD,
        &[HOST, GUEST],
    );
    add(&mut table, 2, "Private Permanent", BATTLEFIELD, &[HOST]);
    table.objects.get_mut(&ObjectId(2)).unwrap().face = Face::Down;
    let mut game = Game::new(table);
    let (token_id, token) = create_public_token(&mut game);
    let select = |id| Selection {
        objects: vec![id],
        zone: BATTLEFIELD,
    };
    let abilities = [
        (
            ObjectId(1),
            Card::Named("Public Permanent".into()),
            "Public ability",
        ),
        (
            ObjectId(2),
            Card::Named("Private Permanent".into()),
            "Private ability",
        ),
        (token_id, token, "Token ability"),
    ];
    let mut markers = Vec::new();
    for (id, _, text) in &abilities {
        let before = game.table.clone();
        let entry = game.act(
            HOST,
            &Action::Marker {
                selection: select(*id),
                text: (*text).into(),
            },
        );
        markers.push(game.table.stack[0]);
        if *id == ObjectId(2) {
            let guest = entry_seen_by(&entry, &before, GUEST);
            let json = serde_json::to_string(&guest).unwrap();
            assert!(!json.contains("Private Permanent"));
            assert!(!json.contains("Private ability"));
        }
    }
    game.act(
        HOST,
        &Action::Move {
            selection: Selection {
                objects: vec![ObjectId(1), ObjectId(2)],
                zone: BATTLEFIELD,
            },
            to: Destination::Zone(HAND),
            position: Position::Top,
            face: None,
            tapped: None,
        },
    );
    game.act(
        HOST,
        &Action::Remove {
            selection: select(token_id),
        },
    );
    game.assert_reopens();
    let (replayed, _) = save::open(&game.file[..]).unwrap();
    for table in [&game.table, &replayed] {
        for seat in table.seats() {
            let view = seen_by(table, seat);
            for (&marker_id, (id, card, text)) in markers.iter().zip(&abilities) {
                let marker = &view.markers[&marker_id];
                assert!(!view.objects.contains_key(id));
                assert_eq!(marker.source, *id);
                if *id == ObjectId(2) && seat == GUEST {
                    assert!(marker.display_source.is_none());
                    assert!(marker.text.is_empty());
                } else {
                    assert_eq!(&marker.display_source.as_ref().unwrap().card, card);
                    assert_eq!(&marker.text, text);
                }
            }
        }
    }
}

#[test]
fn identical_markers_keep_a_borrowed_tokens_controller_and_private_preview() {
    let mut table = Table::new(3);
    let source = ObjectId(1);
    let uninformed = Seat(2);
    let text = "An ability from the back face";
    let card = Card::Named("Borrowed Token".into());
    let mut token = Object::new(source, card.clone(), HOST, BATTLEFIELD);
    token.token = true;
    token.face = Face::Back;
    token.known_to = vec![HOST, GUEST];
    Change::Create {
        object: token,
        index: 0,
    }
    .perform(&mut table);

    let selection = Selection {
        objects: vec![source],
        zone: BATTLEFIELD,
    };
    let mut game = Game::new(table);
    game.act(
        HOST,
        &Action::Give {
            selection: selection.clone(),
            to: GUEST,
        },
    );
    let place = Action::Marker {
        selection: selection.clone(),
        text: text.into(),
    };
    game.act(HOST, &place);
    let first = game.table.stack[0];
    game.act(HOST, &place);
    let second = game.table.stack[0];

    game.act(
        GUEST,
        &Action::Give {
            selection: selection.clone(),
            to: HOST,
        },
    );
    game.act(HOST, &Action::Remove { selection });
    let (reset, _) = save::open(&game.file[..]).unwrap();
    for viewer in [GUEST, uninformed] {
        let view = seen_by(&reset, viewer);
        assert!(!view.objects.contains_key(&source));
        assert_eq!(view.stack, [second, first]);
        for id in view.stack {
            let marker = &view.markers[&id];
            assert_eq!(marker.controller, Some(GUEST));
            if viewer == GUEST {
                let preview = marker.display_source.as_ref().unwrap();
                assert_eq!((&preview.card, preview.face), (&card, Face::Back));
                assert_eq!(marker.text, text);
            } else {
                assert!(marker.display_source.is_none());
                assert!(marker.text.is_empty());
            }
        }
    }

    game.act(
        GUEST,
        &Action::Remove {
            selection: Selection {
                objects: vec![second],
                zone: Zone::Stack,
            },
        },
    );
    game.assert_reopens();
    assert_eq!(game.table.stack, [first]);
    assert_eq!(game.table.markers[&first], reset.markers[&first]);
    assert!(!game.table.markers.contains_key(&second));
}

#[test]
fn an_old_save_header_recovers_a_markers_source_before_replaying_its_removal() {
    let mut table = fixtures();
    let source = ObjectId(1);
    table.objects.get_mut(&source).unwrap().controller = GUEST;
    let marker = ObjectId(10);
    let orphan = ObjectId(11);
    Change::CreateMarker {
        id: orphan,
        marker: AbilityMarker {
            source: ObjectId(9),
            controller: None,
            text: String::new(),
            display_source: None,
        },
        index: 0,
    }
    .perform(&mut table);
    let text = "Ability from an older save";
    Change::CreateMarker {
        id: marker,
        marker: AbilityMarker {
            source,
            text: text.into(),
            controller: None,
            display_source: None,
        },
        index: 0,
    }
    .perform(&mut table);
    let mut game = Game::new(table);
    game.act(HOST, &move_card(source.0, BATTLEFIELD, HAND, Position::Top));

    let (table, _) = save::open(&game.file[..]).unwrap();
    let guest = seen_by(&table, GUEST);
    assert_eq!(
        guest.objects[&guest.zone(HAND).unwrap()[0]].card,
        Card::Hidden
    );
    assert_eq!(guest.markers[&orphan].controller, Some(HOST));
    assert!(guest.markers[&orphan].display_source.is_none());
    let marker = &guest.markers[&marker];
    assert_eq!(marker.controller, Some(GUEST));
    assert_eq!(marker.text, text);
    assert_eq!(
        marker.display_source.as_ref().unwrap().card,
        Card::Named("Underground Sea".into()),
    );
}
