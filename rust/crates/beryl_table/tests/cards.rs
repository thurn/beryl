use beryl_table::action::{
    Adjustment, ComputedDestination, Destination, Position, SeenObject, Selection,
};
use beryl_table::state::{Arrow, ArrowKind, ArrowTarget, Card, Face, Object, ObjectId, Row, Zone};
use beryl_table::{Action, Chance, Change, Entry, Refusal, Seat, Table, apply};

const OWNER: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const BATTLEFIELD: Zone = Zone::Battlefield;
const HAND: Zone = Zone::Hand(OWNER);
const STACK: Zone = Zone::Stack;

fn card(id: u32, name: &str, zone: Zone) -> Object {
    Object::new(ObjectId(id), Card::Named(name.into()), OWNER, zone)
}

fn table_with<const N: usize>(cards: [Object; N]) -> Table {
    let mut table = Table::new(2);
    for object in cards {
        let index = table.zone(object.zone).unwrap().len();
        Change::Create { object, index }.perform(&mut table);
    }
    table
}

fn selection(ids: &[u32], zone: Zone) -> Selection {
    Selection {
        objects: ids.iter().copied().map(ObjectId).collect(),
        zone,
    }
}

fn seen(id: u32, zone: Zone) -> SeenObject {
    SeenObject {
        id: ObjectId(id),
        zone,
    }
}

fn act(table: &mut Table, by: Seat, action: &Action) -> Entry {
    apply(table, by, action, &mut Chance::fresh(0)).unwrap()
}

fn undo(table: &mut Table, entry: &Entry) {
    for change in entry.changes.iter().rev() {
        change.inverse().perform(table);
    }
}

fn object(table: &Table, id: u32) -> &Object {
    &table.objects[&ObjectId(id)]
}

#[test]
fn reorder_places_a_group_in_hand_without_replacing_its_ids() {
    let mut table = table_with([
        card(1, "Island", HAND),
        card(2, "Forest", HAND),
        card(3, "Plains", HAND),
    ]);
    let entry = act(
        &mut table,
        OWNER,
        &Action::Reorder {
            selection: selection(&[3, 1], HAND),
            position: Position::Bottom,
        },
    );
    assert_eq!(entry.n, 1);
    assert_eq!(table.zone(HAND).unwrap(), [2, 3, 1].map(ObjectId));
    undo(&mut table, &entry);
    assert_eq!(table.zone(HAND).unwrap(), [1, 2, 3].map(ObjectId));
}

#[test]
fn pins_gives_and_flags_each_selected_permanent() {
    let mut table = table_with([
        card(1, "Island", BATTLEFIELD),
        card(2, "Forest", BATTLEFIELD),
    ]);
    let selected = selection(&[1, 2], BATTLEFIELD);
    act(
        &mut table,
        OWNER,
        &Action::Pin {
            selection: selected.clone(),
            row: Row::Creatures,
        },
    );
    act(
        &mut table,
        OWNER,
        &Action::Give {
            selection: selected.clone(),
            to: GUEST,
        },
    );
    act(
        &mut table,
        OWNER,
        &Action::Flag {
            selection: selected,
            doesnt_untap: true,
        },
    );
    for card in table.objects.values() {
        assert_eq!(card.row, Row::Creatures);
        assert_eq!(card.controller, GUEST);
        assert_eq!(card.owner, OWNER);
        assert!(card.doesnt_untap);
    }
    let mut solo = Table::new(1);
    Change::Create {
        object: card(1, "Island", BATTLEFIELD),
        index: 0,
    }
    .perform(&mut solo);
    let give = Action::Give {
        selection: selection(&[1], BATTLEFIELD),
        to: OWNER,
    };
    assert_eq!(
        apply(&mut solo, OWNER, &give, &mut Chance::fresh(0)),
        Err(Refusal::INVALID)
    );
}

#[test]
fn turning_cards_down_preserves_knowledge_and_peeking_teaches_only_the_actor() {
    let mut first = card(1, "Fable of the Mirror-Breaker", BATTLEFIELD);
    first.known_to.push(OWNER);
    let mut second = card(2, "Bala Ged Recovery", BATTLEFIELD);
    second.controller = GUEST;
    let mut table = table_with([first, second]);
    let selected = selection(&[1, 2], BATTLEFIELD);
    act(
        &mut table,
        OWNER,
        &Action::Face {
            selection: selected.clone(),
            face: Face::Down,
        },
    );
    assert_eq!(object(&table, 1).known_to, [OWNER]);
    assert!(object(&table, 2).known_to.is_empty());
    let entry = act(
        &mut table,
        GUEST,
        &Action::Peek {
            selection: selection(&[2], BATTLEFIELD),
        },
    );
    undo(&mut table, &entry);
    assert_eq!(object(&table, 1).known_to, [OWNER]);
    assert_eq!(object(&table, 2).known_to, [GUEST]);
    act(
        &mut table,
        OWNER,
        &Action::Face {
            selection: selected,
            face: Face::Back,
        },
    );
    for card in table.objects.values() {
        assert_eq!(card.face, Face::Back);
        assert!(card.known_to.contains(&OWNER));
        assert!(card.known_to.contains(&GUEST));
    }
}

#[test]
fn counters_clamp_at_both_ends_and_remove_zero_counts() {
    let mut table = table_with([
        card(1, "Island", BATTLEFIELD),
        card(2, "Forest", BATTLEFIELD),
    ]);
    for (adjustment, expected) in [
        (Adjustment::Set(20), Some(20)),
        (Adjustment::Add(i32::MAX), Some(999)),
        (Adjustment::Set(i32::MAX), Some(999)),
        (Adjustment::Add(i32::MIN), None),
        (Adjustment::Set(-1), None),
    ] {
        act(
            &mut table,
            OWNER,
            &Action::Counter {
                selection: selection(&[1, 2], BATTLEFIELD),
                name: "charge".into(),
                adjustment,
            },
        );
        for card in table.objects.values() {
            assert_eq!(
                card.counters.first(),
                expected.map(|count| ("charge".into(), count)).as_ref()
            );
        }
    }
}

#[test]
fn stats_damage_and_notes_can_be_set_and_cleared() {
    let mut table = table_with([card(1, "Merfolk Branchwalker", BATTLEFIELD)]);
    let selected = selection(&[1], BATTLEFIELD);
    for (stats, damage, note) in [
        (Some((4, 5)), 3, Some("blocked".to_owned())),
        (None, 0, None),
    ] {
        act(
            &mut table,
            OWNER,
            &Action::Stats {
                selection: selected.clone(),
                stats,
            },
        );
        act(
            &mut table,
            OWNER,
            &Action::Damage {
                selection: selected.clone(),
                damage,
            },
        );
        act(
            &mut table,
            OWNER,
            &Action::Note {
                selection: selected.clone(),
                note: note.clone(),
            },
        );
        let card = object(&table, 1);
        assert_eq!(card.stats, stats);
        assert_eq!(card.damage, damage);
        assert_eq!(card.note, note);
    }
}

#[test]
fn attaches_from_hand_and_battlefield_and_detaches_when_the_host_leaves() {
    let mut table = table_with([
        card(1, "Island", BATTLEFIELD),
        card(2, "Swiftfoot Boots", HAND),
        card(3, "All That Glitters", BATTLEFIELD),
    ]);
    let host = seen(1, BATTLEFIELD);
    act(
        &mut table,
        OWNER,
        &Action::Attach {
            selection: selection(&[2], HAND),
            host,
        },
    );
    act(
        &mut table,
        OWNER,
        &Action::Attach {
            selection: selection(&[3], BATTLEFIELD),
            host,
        },
    );
    assert_eq!(object(&table, 2).zone, BATTLEFIELD);
    for id in [2, 3] {
        assert_eq!(object(&table, id).attached_to, Some(host.id));
    }
    let cycle = Action::Attach {
        selection: selection(&[1], BATTLEFIELD),
        host: seen(2, BATTLEFIELD),
    };
    let before = table.clone();
    assert_eq!(
        apply(&mut table, OWNER, &cycle, &mut Chance::fresh(0)),
        Err(Refusal::INVALID)
    );
    assert_eq!(table, before);
    let stale = Action::Attach {
        selection: selection(&[3], BATTLEFIELD),
        host: seen(2, HAND),
    };
    assert_eq!(
        apply(&mut table, OWNER, &stale, &mut Chance::fresh(0)),
        Err(Refusal::Stale)
    );
    assert_eq!(table, before);

    act(
        &mut table,
        OWNER,
        &Action::Unattach {
            selection: selection(&[3], BATTLEFIELD),
        },
    );
    assert_eq!(object(&table, 3).attached_to, None);
    act(
        &mut table,
        OWNER,
        &movement(&[1], BATTLEFIELD, Destination::Zone(Zone::Graveyard(OWNER))),
    );
    assert_eq!(object(&table, 2).zone, BATTLEFIELD);
    assert_eq!(object(&table, 2).attached_to, None);
}

#[test]
fn clone_creates_fresh_tokens_with_the_printed_identity_and_no_marked_damage() {
    let mut first = card(1, "Merfolk Branchwalker", BATTLEFIELD);
    first.damage = 1;
    first.tapped = true;
    first.counters.push(("charge".into(), 3));
    let mut table = table_with([first, card(2, "Forest", BATTLEFIELD)]);
    let entry = act(
        &mut table,
        GUEST,
        &Action::Clone {
            selection: selection(&[2, 1], BATTLEFIELD),
        },
    );
    let copies = &table.battlefield[..2];
    assert_eq!(table.objects[&copies[0]].card, object(&table, 2).card);
    assert_eq!(table.objects[&copies[1]].card, object(&table, 1).card);
    for id in copies {
        let copy = &table.objects[id];
        assert!(copy.token);
        assert_eq!(copy.owner, GUEST);
        assert_eq!(copy.controller, GUEST);
        assert!(!copy.tapped);
        assert_eq!(copy.damage, 0);
        assert!(copy.counters.is_empty());
        assert_eq!(copy.known_to, [OWNER, GUEST]);
    }
    let removed_ids = copies.to_vec();
    undo(&mut table, &entry);
    assert_eq!(table.battlefield, [1, 2].map(ObjectId));
    act(
        &mut table,
        GUEST,
        &Action::Clone {
            selection: selection(&[2, 1], BATTLEFIELD),
        },
    );
    assert!(removed_ids.iter().all(|id| !table.objects.contains_key(id)));
}

#[test]
fn creates_named_and_custom_tokens_with_a_host_and_note() {
    let mut table = table_with([card(1, "Island", BATTLEFIELD)]);
    let host = seen(1, BATTLEFIELD);
    let custom = Card::Custom {
        name: "Wisp".into(),
        types: "Creature".into(),
        colors: vec!["U".into()],
        stats: Some((1, 2)),
    };
    for card in [Card::Named("Food Token".into()), custom] {
        act(
            &mut table,
            GUEST,
            &Action::Token {
                card: card.clone(),
                count: 2,
                attached_to: Some(host),
                note: Some("test note".into()),
            },
        );
        for id in &table.battlefield[..2] {
            let token = &table.objects[id];
            assert_eq!(token.card, card);
            assert_eq!(token.attached_to, Some(host.id));
            assert_eq!(token.note.as_deref(), Some("test note"));
            assert_eq!(token.known_to, [OWNER, GUEST]);
            assert!(token.token);
        }
    }
}

#[test]
fn marker_creates_one_stack_entry_per_source_and_can_be_removed() {
    let mut table = table_with([
        card(1, "Island", BATTLEFIELD),
        card(2, "Forest", BATTLEFIELD),
        card(3, "Force of Will", STACK),
    ]);
    act(
        &mut table,
        OWNER,
        &Action::Marker {
            selection: selection(&[2, 1], BATTLEFIELD),
            text: "Ability".into(),
        },
    );
    let markers = table.stack[..2].to_vec();
    for (&id, source) in markers.iter().zip([2, 1]) {
        assert_eq!(table.markers[&id].source, ObjectId(source));
        assert_eq!(table.markers[&id].text, "Ability");
    }
    let remove = Action::Remove {
        selection: Selection {
            objects: markers,
            zone: STACK,
        },
    };
    let before = table.clone();
    let entry = act(&mut table, OWNER, &remove);
    assert!(table.markers.is_empty());
    assert_eq!(table.stack, [ObjectId(3)]);
    undo(&mut table, &entry);
    assert_eq!(table.markers, before.markers);
    assert_eq!(table.stack, before.stack);
}

#[test]
fn removing_a_token_cleans_up_attachments_and_arrows_and_refuses_cards() {
    let mut token = card(1, "Food Token", BATTLEFIELD);
    token.token = true;
    let mut attached = card(2, "Swiftfoot Boots", BATTLEFIELD);
    attached.attached_to = Some(token.id);
    let mut table = table_with([token, attached]);
    table.arrows.push(Arrow {
        from: ObjectId(1),
        to: ArrowTarget::Object(ObjectId(2)),
        by: OWNER,
        kind: ArrowKind::FriendlyTarget,
    });
    let before = table.clone();
    let remove = Action::Remove {
        selection: selection(&[1, 2], BATTLEFIELD),
    };
    assert_eq!(
        apply(&mut table, OWNER, &remove, &mut Chance::fresh(0)),
        Err(Refusal::INVALID)
    );
    assert_eq!(table, before);

    let entry = act(
        &mut table,
        OWNER,
        &Action::Remove {
            selection: selection(&[1], BATTLEFIELD),
        },
    );
    assert_eq!(table.battlefield, [ObjectId(2)]);
    assert!(table.arrows.is_empty());
    assert_eq!(object(&table, 2).attached_to, None);
    undo(&mut table, &entry);
    assert_eq!(table.objects, before.objects);
    assert_eq!(table.battlefield, before.battlefield);
    assert_eq!(table.arrows, before.arrows);
}

fn movement(ids: &[u32], zone: Zone, to: Destination) -> Action {
    Action::Move {
        selection: selection(ids, zone),
        to,
        position: Position::Top,
        face: None,
        tapped: None,
    }
}

#[test]
fn play_and_resolve_use_printed_facts_and_preserve_selection_order() {
    let mut instant = card(3, "Force of Will", HAND);
    instant.owner = GUEST;
    let mut table = table_with([
        card(1, "Temple of Epiphany", HAND),
        card(2, "Merfolk Branchwalker", HAND),
        instant,
        card(4, "Jace, the Mind Sculptor", HAND),
        card(5, "Invasion of Amonkhet", HAND),
        card(6, "Glacial Fortress", HAND),
    ]);
    let play = Destination::Computed(ComputedDestination::Play);
    let resolve = Destination::Computed(ComputedDestination::Resolve);
    let entry = act(
        &mut table,
        OWNER,
        &movement(&[2, 1, 3, 4, 5, 6], HAND, play),
    );
    assert_eq!(entry.n, 1);
    assert_eq!(table.battlefield, [1, 6].map(ObjectId));
    assert!(object(&table, 1).tapped);
    assert!(!object(&table, 6).tapped);
    assert_eq!(table.stack, [2, 3, 4, 5].map(ObjectId));
    act(
        &mut table,
        OWNER,
        &Action::Marker {
            selection: selection(&[1], BATTLEFIELD),
            text: "Ability".into(),
        },
    );
    let marker = table.stack[0];
    let before = table.clone();
    let entry = act(
        &mut table,
        OWNER,
        &movement(&[marker.0, 4, 3, 5, 2], STACK, resolve),
    );
    assert!(table.stack.is_empty());
    assert!(table.markers.is_empty());
    assert_eq!(table.battlefield, [4, 5, 2, 1, 6].map(ObjectId));
    assert_eq!(table.zone(Zone::Graveyard(GUEST)).unwrap(), [ObjectId(3)]);
    assert_eq!(object(&table, 4).counters, [("loyalty".into(), 3)]);
    assert_eq!(object(&table, 5).counters, [("defense".into(), 4)]);
    undo(&mut table, &entry);
    assert_eq!(table.stack, before.stack);
    assert_eq!(table.objects, before.objects);
    assert_eq!(table.markers, before.markers);
    assert_eq!(table.battlefield, before.battlefield);
}

#[test]
fn play_respects_the_chosen_back_face_and_a_face_down_play_reveals_nothing() {
    let mut table = table_with([
        card(1, "Bala Ged Recovery", HAND),
        card(2, "Jace, the Mind Sculptor", HAND),
        card(3, "Invasion of Amonkhet", HAND),
    ]);
    for (id, face) in [(1, Face::Back), (2, Face::Down)] {
        act(
            &mut table,
            OWNER,
            &Action::Move {
                selection: selection(&[id], HAND),
                to: Destination::Computed(ComputedDestination::Play),
                position: Position::Top,
                face: Some(face),
                tapped: None,
            },
        );
        assert_eq!(object(&table, id).zone, BATTLEFIELD);
        assert_eq!(object(&table, id).face, face);
    }
    assert!(object(&table, 1).tapped);
    assert!(object(&table, 2).known_to.is_empty());
    assert!(object(&table, 2).counters.is_empty());
    act(
        &mut table,
        OWNER,
        &movement(&[3], HAND, Destination::Zone(BATTLEFIELD)),
    );
    assert_eq!(object(&table, 3).counters, [("defense".into(), 4)]);
}

#[test]
fn wire_actions_play_a_face_down_card_and_peek_without_revealing_it() {
    let mut table = table_with([card(1, "Forest", HAND)]);
    for wire in [
        r#"{"move":{"objects":[1],"in":{"hand":0},"to":"play","face":"down"}}"#,
        r#"{"peek":{"objects":[1],"in":"battlefield"}}"#,
    ] {
        let action = serde_json::from_str(wire).unwrap();
        act(&mut table, OWNER, &action);
    }
    let card = object(&table, 1);
    assert_eq!(card.zone, BATTLEFIELD);
    assert_eq!(card.face, Face::Down);
    assert_eq!(card.known_to, [OWNER]);
}
