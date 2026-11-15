use beryl_session::card_records::{CardRecordsRequest, card_records};
use beryl_session::protocol::{
    Counter, FaceName, RowName, Side, StackItem, Status, Step, TablePhase,
};
use beryl_session::script::{Arrow, Subject};
use beryl_table::action::{Destination, Portion, Position, Selection};
use beryl_table::state::{
    Arrow as TableArrow, ArrowKind, ArrowTarget, Assist, Card, Face, Object, ObjectId, Phase, Row,
    Zone,
};
use beryl_table::testing::{CORE, Generator};
use beryl_table::{Action, Chance, Change, Log, Seat, Table};
use beryl_table_session::view::{Slots, view};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn add(table: &mut Table, id: u32, name: &str, zone: Zone, known_to: &[Seat]) -> ObjectId {
    let id = ObjectId(id);
    let mut object = Object::new(id, Card::Named(name.into()), HOST, zone);
    object.known_to = known_to.to_vec();
    Change::Create { object, index: 0 }.perform(table);
    id
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one fixture verifies every zone from both seats"
)]
fn both_seats_get_ordered_piles_and_relative_table_state() {
    let mut table = Table::new(2);
    let bottom = add(&mut table, 1, "Island", Zone::Library(HOST), &[]);
    let top = add(&mut table, 2, "Plains", Zone::Library(HOST), &[HOST]);
    let hand = add(&mut table, 3, "Ponder", Zone::Hand(GUEST), &[GUEST]);
    let graveyard = add(&mut table, 4, "Opt", Zone::Graveyard(HOST), &[HOST, GUEST]);
    let exile = add(&mut table, 5, "Consider", Zone::Exile(HOST), &[HOST, GUEST]);
    let sideboard = add(&mut table, 6, "Negate", Zone::Sideboard(HOST), &[HOST]);
    let land = add(&mut table, 7, "Island", Zone::Battlefield, &[HOST, GUEST]);
    let creature = add(
        &mut table,
        8,
        "Savannah Lions",
        Zone::Battlefield,
        &[HOST, GUEST],
    );
    let spell = add(&mut table, 9, "Counterspell", Zone::Stack, &[HOST, GUEST]);
    let permanent = table.objects.get_mut(&creature).unwrap();
    permanent.tapped = true;
    permanent.token = true;
    permanent.damage = 1;
    permanent.note = Some("untap next turn".into());
    permanent.stats = Some((5, 3));
    permanent.attached_to = Some(land);
    permanent.row = Row::Others;
    permanent.counters = vec![
        ("+1/+1".into(), 300),
        ("charge".into(), 2),
        ("custom".into(), 1),
    ];
    table.active = GUEST;
    table.turn = 4;
    table.phase = Phase::SecondMain;
    table.players[0].life = 12;
    table.players[0].counters = ["W", "U", "B", "R", "G", "C", "energy"]
        .into_iter()
        .enumerate()
        .map(|(n, name)| (name.into(), n as i32 + 1))
        .collect();
    table.assists[0] = vec![Assist::Combat];
    table.lent_zones.push((Zone::Library(HOST), GUEST));
    table.arrows.push(TableArrow {
        from: creature,
        to: ArrowTarget::Player(GUEST),
        by: HOST,
        kind: ArrowKind::Attack,
    });
    let mut log = Log::new(&table);
    log.apply(
        &mut table,
        HOST,
        &Action::Marker {
            selection: Selection {
                objects: vec![creature],
                zone: Zone::Battlefield,
            },
            text: "An ability".into(),
        },
        &mut Chance::fresh(1),
    )
    .unwrap();

    for seat in [HOST, GUEST] {
        let mut slots = Slots::default();
        let (view, refs) = view(&table, &log, seat, &mut slots).unwrap();
        let slot = |id| slots.get(id).unwrap();
        let object = |id| {
            view.objects
                .iter()
                .find(|object| object.slot == slot(id))
                .unwrap()
        };
        let manual = view.table.as_ref().unwrap();
        let (host, host_piles, guest_piles, guest_side) = if seat == HOST {
            (
                &view.players.you,
                &manual.orders.you,
                &manual.orders.opponent,
                Side::Opponent,
            )
        } else {
            (
                &view.players.opponent,
                &manual.orders.opponent,
                &manual.orders.you,
                Side::You,
            )
        };
        assert_eq!(view.status, Status::Free);
        assert_eq!(view.turn.active, guest_side);
        assert_eq!(
            (view.turn.number, view.turn.own_number, view.turn.step),
            (4, 2, Step::PostcombatMain)
        );
        assert_eq!(manual.phase, TablePhase::SecondMain);
        assert_eq!(manual.seats, 2);
        assert_eq!(manual.seat.unwrap_or_default(), seat.0);
        assert_eq!(host.life, 12);
        assert_eq!(
            [
                host.mana.white,
                host.mana.blue,
                host.mana.black,
                host.mana.red,
                host.mana.green,
                host.mana.colorless
            ],
            [1, 2, 3, 4, 5, 6]
        );
        assert_eq!(host.named_counters[0].name, "energy");
        assert_eq!(host.library, [slot(bottom), slot(top)]);
        assert_eq!(host.graveyard, [slot(graveyard)]);
        assert_eq!(host_piles.exile, [slot(exile)]);
        assert_eq!(host_piles.sideboard, [slot(sideboard)]);
        assert_eq!(guest_piles.hand, [slot(hand)]);
        assert_eq!(object(hand).zone_side, Some(guest_side));
        assert_eq!(object(hand).name.is_some(), seat == GUEST);
        assert_eq!(object(top).name.is_some(), seat == HOST);
        assert_eq!(object(sideboard).name.is_some(), seat == HOST);
        assert!(object(bottom).name.is_none());
        assert_eq!(view.battlefield, [slot(creature), slot(land)]);
        assert_eq!(object(land).pinned, Some(RowName::Lands));
        let permanent = object(creature);
        assert_eq!(permanent.pinned, Some(RowName::Others));
        assert_eq!(permanent.attached_to, Some(slot(land)));
        assert!(permanent.token && permanent.tapped);
        assert_eq!(permanent.damage, 1);
        assert_eq!(permanent.pt.unwrap().power, 305);
        assert_eq!(permanent.counters[0].kind, Counter::PlusOne);
        assert_eq!(permanent.counters[1].kind, Counter::Charge);
        assert_eq!(permanent.named_counters[0].name, "custom");
        assert_eq!(permanent.note.as_deref(), Some("untap next turn"));
        assert_eq!(view.stack[0], StackItem::spell(slot(spell)));
        assert!(
            matches!(&view.stack[1], StackItem::Ability { source: Some(source), name, text: Some(text), .. }
            if *source == slot(creature) && name == "Savannah Lions" && text == "An ability")
        );
        assert_eq!(manual.arrows[0].to, Subject::Player { player: guest_side });
        assert_eq!(manual.arrows[0].arrow, Arrow::Attack);
        assert_eq!(manual.assists.combat, seat == HOST);
        assert_eq!(
            manual.log[0].by,
            if seat == HOST { "You" } else { "Seat 1" }
        );
        assert_eq!(manual.log[0].text, "Put an ability on the stack.");
        assert_eq!(refs.actions(), manual.object_actions);
        assert_eq!(manual.acting.len(), usize::from(seat == GUEST));
        let action = |id| {
            refs.actions()
                .iter()
                .find(|entry| entry.subject == Subject::Object { slot: slot(id) })
        };
        assert!(
            action(bottom).is_some(),
            "the card back is usable in a lent library"
        );
        assert_eq!(action(sideboard).is_some(), seat == HOST);
        assert!(refs.actions().iter().any(|entry| entry.subject
            == Subject::Ability {
                id: view.stack[1].marker_id().unwrap()
            }));
    }
}

#[test]
fn only_the_seat_that_knows_the_draw_can_follow_its_slot() {
    let mut table = Table::new(2);
    let before = add(
        &mut table,
        100,
        "Brainstorm",
        Zone::Library(HOST),
        &[HOST, GUEST],
    );
    let mut log = Log::new(&table);
    let mut owner_slots = Slots::default();
    let mut guest_slots = Slots::default();
    view(&table, &log, HOST, &mut owner_slots).unwrap();
    view(&table, &log, GUEST, &mut guest_slots).unwrap();
    let owner_before = owner_slots.get(before);
    let guest_before = guest_slots.get(before);
    let mut chance = Chance::fresh(10);
    log.apply(
        &mut table,
        HOST,
        &Action::Draw {
            seat: HOST,
            count: 1,
        },
        &mut chance,
    )
    .unwrap();

    let after = table.zones[0].hand[0];
    assert_ne!(before, after);
    let (owner, refs) = view(&table, &log, HOST, &mut owner_slots).unwrap();
    let (guest, _) = view(&table, &log, GUEST, &mut guest_slots).unwrap();
    assert_eq!(owner_slots.get(after), owner_before);
    assert_ne!(guest_slots.get(after), guest_before);
    assert_eq!(owner.objects[0].name.as_deref(), Some("Brainstorm"));
    assert!(guest.objects[0].name.is_none());
    assert!(
        !serde_json::to_string(&guest)
            .unwrap()
            .contains("Brainstorm")
    );
    assert!(
        refs.actions()
            .iter()
            .any(|entry| entry.zone == Zone::Hand(HOST))
    );

    log.apply(
        &mut table,
        HOST,
        &Action::Undo { entry: Some(1) },
        &mut chance,
    )
    .unwrap();
    view(&table, &log, HOST, &mut owner_slots).unwrap();
    view(&table, &log, GUEST, &mut guest_slots).unwrap();
    assert_eq!(owner_slots.get(before), owner_before);
    assert_ne!(guest_slots.get(before), guest_before);
    let unshuffled = owner_slots.get(before);
    log.apply(
        &mut table,
        HOST,
        &Action::Shuffle {
            seat: HOST,
            portion: Portion::All,
        },
        &mut chance,
    )
    .unwrap();
    let (shuffled, _) = view(&table, &log, HOST, &mut owner_slots).unwrap();
    assert_ne!(Some(shuffled.objects[0].slot), unshuffled);
    assert!(shuffled.objects[0].name.is_none());
}

#[test]
fn a_solo_view_uses_the_card_record_of_the_face_up() {
    let mut table = Table::new(1);
    let id = add(
        &mut table,
        1,
        "Sink into Stupor",
        Zone::Battlefield,
        &[HOST],
    );
    table.objects.get_mut(&id).unwrap().face = Face::Back;
    let (view, _) = view(&table, &Log::new(&table), HOST, &mut Slots::default()).unwrap();
    let card = &view.objects[0];
    assert_eq!(card.face, FaceName::Back);
    assert_eq!(card.name.as_deref(), Some("Soporific Springs"));
    assert_eq!(card.pinned, Some(RowName::Lands));
    let records = card_records(&CardRecordsRequest {
        names: vec![card.name.clone().unwrap()],
    });
    let face = &records.cards["Soporific Springs"].faces[1];
    assert_eq!(face.type_line, "Land");
    assert!(face.oracle_text.as_ref().unwrap().contains("Add"));
    assert!(face.images.normal.contains("/back/"));
    assert!(view.players.opponent.library.is_empty());
    assert!(view.table.unwrap().orders.opponent.hand.is_empty());
}

#[test]
fn a_marker_keeps_its_redacted_preview_after_its_source_leaves() {
    let mut table = Table::new(2);
    let secret = add(&mut table, 1, "Brazen Borrower", Zone::Battlefield, &[HOST]);
    table.objects.get_mut(&secret).unwrap().face = Face::Down;
    let selection = Selection {
        objects: vec![secret],
        zone: Zone::Battlefield,
    };
    let mut log = Log::new(&table);
    let mut chance = Chance::fresh(10);
    for action in [
        Action::Marker {
            selection: selection.clone(),
            text: "Private text".into(),
        },
        Action::Move {
            selection,
            to: Destination::Zone(Zone::Hand(HOST)),
            position: Position::Top,
            face: None,
            tapped: None,
        },
    ] {
        log.apply(&mut table, HOST, &action, &mut chance).unwrap();
    }
    let (owner, _) = view(&table, &log, HOST, &mut Slots::default()).unwrap();
    let (guest, _) = view(&table, &log, GUEST, &mut Slots::default()).unwrap();
    assert!(
        matches!(&owner.stack[0], StackItem::Ability { source: None, name, text: Some(text), .. }
        if name == "Brazen Borrower" && text == "Private text")
    );
    assert!(
        matches!(&guest.stack[0], StackItem::Ability { source: None, name, text: Some(text), .. }
        if name.is_empty() && text.is_empty())
    );
}

#[test]
fn one_thousand_random_actions_keep_unknown_names_out_of_each_view() {
    let mut table = Table::new(2);
    let mut generator = Generator::new(227);
    let mut chance = Chance::fresh(481);
    let mut log = Log::new(&table);
    let mut slots = [Slots::default(), Slots::default()];
    for index in 0..1_000 {
        let (_, by, action) = generator.next(&table, CORE);
        log.apply(&mut table, by, &action, &mut chance).unwrap();
        for seat in table.seats() {
            let slots = &mut slots[usize::from(seat.0)];
            let (view, _) = view(&table, &log, seat, slots).unwrap();
            let json = serde_json::to_string(&view).unwrap();
            for object in table.objects.values() {
                let (Card::Named(name) | Card::Custom { name, .. }) = &object.card else {
                    panic!("host is missing a card identity");
                };
                let shown = view
                    .objects
                    .iter()
                    .find(|shown| Some(shown.slot) == slots.get(object.id))
                    .unwrap();
                if object.known_to.contains(&seat) {
                    assert_eq!(shown.name.as_ref(), Some(name));
                } else {
                    assert!(shown.name.is_none());
                    assert!(
                        !json.contains(name),
                        "action {index} leaked {name} to {seat:?}: {action:?}, {:?}, {:?}",
                        view.stack,
                        view.table
                            .as_ref()
                            .unwrap()
                            .log
                            .iter()
                            .filter(|row| row.text.contains(name))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}
