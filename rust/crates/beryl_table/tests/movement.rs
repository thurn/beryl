use beryl_table::action::{Destination, Position, Selection};
use beryl_table::state::{Arrow, ArrowKind, ArrowTarget, Card, Face, Object, ObjectId, Row, Zone};
use beryl_table::{Action, Chance, Change, Refusal, Seat, Table, apply};

const OWNER: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn card(id: u32, zone: Zone) -> Object {
    Object::new(
        ObjectId(id),
        Card::Named(format!("test card {id}")),
        OWNER,
        zone,
    )
}

fn table_with(cards: impl IntoIterator<Item = Object>) -> Table {
    let mut table = Table::new(2);
    for object in cards {
        let index = table.zone(object.zone).unwrap().len();
        Change::Create { object, index }.perform(&mut table);
    }
    table
}

fn movement(objects: &[u32], from: Zone, to: Zone, position: Position) -> Action {
    Action::Move {
        selection: selection(objects, from),
        to: Destination::Zone(to),
        position,
        face: None,
        tapped: None,
    }
}

fn selection(objects: &[u32], zone: Zone) -> Selection {
    Selection {
        objects: objects.iter().copied().map(ObjectId).collect(),
        zone,
    }
}

#[test]
fn moves_to_every_zone_and_position_in_the_requested_order() {
    for to in [
        Zone::Library(OWNER),
        Zone::Hand(OWNER),
        Zone::Graveyard(OWNER),
        Zone::Exile(OWNER),
        Zone::Sideboard(OWNER),
        Zone::Battlefield,
        Zone::Stack,
    ] {
        let from = Zone::Library(GUEST);
        for (position, index) in [
            (Position::Top, 0),
            (Position::Nth(1), 1),
            (Position::Bottom, 2),
        ] {
            let mut table = table_with([card(1, from), card(2, from), card(3, to), card(4, to)]);
            let action = movement(&[1, 2], from, to, position);
            let entry = apply(&mut table, OWNER, &action, &mut Chance::fresh(0)).unwrap();
            assert_eq!(entry.n, 1);
            assert_eq!(entry.action, action);
            assert_eq!(table.zone(from), Some([].as_slice()));
            let cards: Vec<_> = table
                .zone(to)
                .unwrap()
                .iter()
                .map(|id| &table.objects[id].card)
                .collect();
            let mut expected = vec![card(3, to).card, card(4, to).card];
            expected.splice(index..index, [card(1, to).card, card(2, to).card]);
            assert_eq!(cards, expected.iter().collect::<Vec<_>>());
        }
    }
}

#[test]
fn repositions_a_noncontiguous_group_without_refreshing_its_hand_ids() {
    let zone = Zone::Hand(OWNER);
    let mut table = table_with((1..=5).map(|id| card(id, zone)));
    let before = table.zone(zone).unwrap().to_vec();
    let entry = apply(
        &mut table,
        OWNER,
        &movement(&[5, 1], zone, zone, Position::Nth(1)),
        &mut Chance::fresh(0),
    )
    .unwrap();
    assert_eq!(table.zone(zone).unwrap(), [2, 5, 1, 3, 4].map(ObjectId));
    for change in entry.changes.iter().rev() {
        change.inverse().perform(&mut table);
    }
    assert_eq!(table.zone(zone).unwrap(), before);
}

#[test]
fn public_face_down_cards_keep_their_visibility_and_can_arrive_tapped() {
    let from = Zone::Library(OWNER);
    let mut table = table_with([card(1, from), card(2, from)]);
    table
        .objects
        .get_mut(&ObjectId(1))
        .unwrap()
        .known_to
        .push(OWNER);
    let zone = Zone::Battlefield;
    for (id, face, known_to) in [
        (1, Face::Down, vec![OWNER]),
        (2, Face::Back, vec![OWNER, GUEST]),
    ] {
        let action = Action::Move {
            selection: selection(&[id], from),
            to: Destination::Zone(zone),
            position: Position::Top,
            face: Some(face),
            tapped: Some(true),
        };
        apply(&mut table, OWNER, &action, &mut Chance::fresh(0)).unwrap();
        let moved = &table.objects[&ObjectId(id)];
        assert_eq!(moved.face, face);
        assert!(moved.tapped);
        assert_eq!(moved.known_to, known_to);
    }
}

#[test]
fn leaving_battlefield_clears_state_detaches_cards_and_removes_touching_arrows() {
    let from = Zone::Battlefield;
    let to = Zone::Exile(GUEST);
    let mut leaving = card(1, from);
    leaving.controller = GUEST;
    leaving.tapped = true;
    leaving.row = Row::Lands;
    leaving.counters = vec![("charge".into(), 3)];
    leaving.stats = Some((9, 9));
    leaving.damage = 5;
    leaving.note = Some("test note".into());
    leaving.doesnt_untap = true;
    leaving.attached_to = Some(ObjectId(3));
    leaving.known_to = vec![OWNER, GUEST];
    let mut attachment = card(2, from);
    attachment.attached_to = Some(ObjectId(1));
    let mut table = table_with([leaving, attachment, card(3, from)]);
    table.arrows = [(1, 2), (2, 3), (3, 1)]
        .into_iter()
        .map(|(from, to)| Arrow {
            from: ObjectId(from),
            to: ArrowTarget::Object(ObjectId(to)),
            by: OWNER,
            kind: ArrowKind::FriendlyTarget,
        })
        .collect();
    let before = table.clone();

    let entry = apply(
        &mut table,
        OWNER,
        &movement(&[1], from, to, Position::Top),
        &mut Chance::fresh(0),
    )
    .unwrap();
    let mut expected = card(1, to);
    expected.known_to = vec![OWNER, GUEST];
    assert_eq!(table.objects[&ObjectId(1)], expected);
    assert_eq!(table.objects[&ObjectId(2)].attached_to, None);
    assert_eq!(table.arrows, vec![before.arrows[1].clone()]);
    for change in entry.changes.iter().rev() {
        change.inverse().perform(&mut table);
    }
    assert_eq!(table.objects, before.objects);
    assert_eq!(table.battlefield, before.battlefield);
    assert_eq!(table.arrows, before.arrows);
}

#[test]
fn a_token_leaves_only_for_the_stack_and_keeps_its_note_there() {
    let from = Zone::Battlefield;
    let mut token = card(1, from);
    token.token = true;
    token.note = Some("test note".into());
    token.attached_to = Some(ObjectId(2));
    let mut table = table_with([token.clone(), card(2, from)]);
    let mut chance = Chance::fresh(0);
    let to = Zone::Library(OWNER);
    let entry = apply(
        &mut table,
        OWNER,
        &movement(&[1], from, to, Position::Top),
        &mut chance,
    )
    .unwrap();
    assert!(!table.objects.contains_key(&ObjectId(1)));
    assert!(table.zone(to).unwrap().is_empty());
    for change in entry.changes.iter().rev() {
        change.inverse().perform(&mut table);
    }
    assert_eq!(table.objects[&ObjectId(1)], token);
    apply(
        &mut table,
        OWNER,
        &movement(&[1], from, Zone::Stack, Position::Top),
        &mut chance,
    )
    .unwrap();
    assert_eq!(table.stack, [ObjectId(1)]);
    assert_eq!(table.objects[&ObjectId(1)].note, token.note);
    assert_eq!(table.objects[&ObjectId(1)].attached_to, None);
}

#[test]
fn hand_entries_refresh_ids_and_hide_cards_and_undo_merges_knowledge() {
    let from = Zone::Library(OWNER);
    let to = Zone::Hand(GUEST);
    let mut first = card(1, from);
    first.known_to = vec![OWNER];
    let mut table = table_with([first, card(2, from)]);
    let entry = apply(
        &mut table,
        GUEST,
        &movement(&[1, 2], from, to, Position::Top),
        &mut Chance::fresh(0),
    )
    .unwrap();
    let hand = table.zone(to).unwrap();
    assert_ne!(hand[0], ObjectId(1));
    assert_ne!(hand[1], ObjectId(2));
    assert!(hand.iter().all(|id| table.objects[id].known_to == [GUEST]));
    for change in entry.changes.iter().rev() {
        change.inverse().perform(&mut table);
    }
    assert_eq!(table.zone(from).unwrap(), [ObjectId(1), ObjectId(2)]);
    assert_eq!(table.objects[&ObjectId(1)].known_to, [OWNER, GUEST]);
    assert_eq!(table.objects[&ObjectId(2)].known_to, [GUEST]);
}

#[test]
fn libraries_keep_visibility_and_sideboards_add_their_owner() {
    let from = Zone::Battlefield;
    let to = Zone::Library(GUEST);
    let mut first = card(1, from);
    first.known_to = vec![OWNER];
    let mut table = table_with([first]);
    let mut chance = Chance::fresh(0);
    apply(
        &mut table,
        OWNER,
        &movement(&[1], from, to, Position::Top),
        &mut chance,
    )
    .unwrap();
    assert_eq!(table.objects[&ObjectId(1)].known_to, [OWNER]);
    apply(
        &mut table,
        OWNER,
        &movement(&[1], to, Zone::Sideboard(GUEST), Position::Top),
        &mut chance,
    )
    .unwrap();
    assert_eq!(table.objects[&ObjectId(1)].known_to, [OWNER, GUEST]);
}

#[test]
fn tapping_sets_every_selected_card_and_invalid_actions_are_atomic() {
    let zone = Zone::Battlefield;
    let mut table = table_with([card(1, zone), card(2, zone), card(3, Zone::Hand(GUEST))]);
    let mut chance = Chance::fresh(0);
    for tapped in [true, false] {
        let action = Action::Tap {
            selection: selection(&[1, 2], zone),
            tapped,
        };
        let entry = apply(&mut table, OWNER, &action, &mut chance).unwrap();
        assert_eq!(entry.changes.len(), 2);
        assert!(
            table
                .battlefield
                .iter()
                .all(|id| table.objects[id].tapped == tapped)
        );
    }
    let before = table.clone();
    for (action, refusal) in [
        (
            Action::Tap {
                selection: selection(&[1, 3], zone),
                tapped: true,
            },
            Refusal::Stale,
        ),
        (
            movement(&[1, 1], zone, Zone::Stack, Position::Top),
            Refusal::Invalid,
        ),
        (
            movement(&[1], zone, Zone::Stack, Position::Nth(1)),
            Refusal::Invalid,
        ),
        (Action::NextTurn {}, Refusal::Invalid),
    ] {
        assert_eq!(apply(&mut table, OWNER, &action, &mut chance), Err(refusal));
        assert_eq!(table, before);
    }
}
