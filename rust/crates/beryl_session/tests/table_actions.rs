use beryl_session::protocol::{
    ObjectId as Slot, TableAction, TableActionRefs, TableObjectAction, TableObjectRef,
};
use beryl_session::script::Subject;
use beryl_table::action::{
    ComputedDestination, Destination, Portion, Position, SeenObject, Selection, Target,
};
use beryl_table::state::{ArrowTarget, Card, Object, ObjectId, Zone};
use beryl_table::{Action, Chance, Change, Log, Refusal, Seat, Table};

const OWNER: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const LIBRARY: Zone = Zone::Library(OWNER);
const HAND: Zone = Zone::Hand(OWNER);
const FIELD: Zone = Zone::Battlefield;

fn object(slot: u16) -> Subject {
    Subject::Object { slot: Slot(slot) }
}

fn put(table: &mut Table, id: u32, zone: Zone) -> ObjectId {
    let id = ObjectId(id);
    Change::Create {
        object: Object::new(id, Card::Named("Island".into()), OWNER, zone),
        index: table.zone(zone).unwrap().len(),
    }
    .perform(table);
    id
}

fn publish(table: &Table, seat: Seat, objects: &[(Subject, ObjectId)]) -> TableActionRefs {
    let mut view = TableActionRefs::new(table, seat);
    for &(subject, id) in objects {
        view.expose(table, subject, id);
    }
    view
}

fn select(item: &TableObjectAction) -> Selection<TableObjectRef> {
    Selection {
        objects: vec![item.reference.clone()],
        zone: item.zone,
    }
}

fn move_to(item: &TableObjectAction, to: Destination) -> TableAction {
    Action::Move {
        selection: select(item),
        to,
        position: Position::Top,
        face: None,
        tapped: None,
    }
}

#[test]
fn a_drawn_card_can_be_played_tapped_and_targeted_through_the_view() {
    let mut table = Table::new(2);
    let library_id = put(&mut table, 1000, LIBRARY);
    let source = put(&mut table, 1, FIELD);
    let mut log = Log::new(&table);
    let mut chance = Chance::fresh(42);
    let library = publish(&table, OWNER, &[(object(7), library_id)]);
    library
        .apply(
            &mut table,
            &mut log,
            OWNER,
            Action::Draw {
                seat: OWNER,
                count: 1,
            },
            &mut chance,
        )
        .unwrap();
    let drawn = table.zone(HAND).unwrap()[0];
    assert_ne!(drawn, library_id);
    let hand = publish(&table, OWNER, &[(object(7), drawn)]);
    let play = Destination::Computed(ComputedDestination::Play);
    assert_eq!(
        hand.apply(
            &mut table,
            &mut log,
            OWNER,
            move_to(&library.actions()[0], play),
            &mut chance
        ),
        Err(Refusal::Stale)
    );
    // Exercise the command's JSON shape as received over IPC or the link.
    let payload = serde_json::to_value(move_to(&hand.actions()[0], play)).unwrap();
    assert!(payload["move"]["objects"][0].is_object());
    let mut host_payload = payload.clone();
    host_payload["move"]["objects"][0] = serde_json::json!(drawn.0);
    assert!(serde_json::from_value::<TableAction>(host_payload).is_err());
    let entry = hand
        .apply(
            &mut table,
            &mut log,
            OWNER,
            serde_json::from_value(payload).unwrap(),
            &mut chance,
        )
        .unwrap();
    assert!(
        matches!(&entry.action, Action::Move { selection, .. } if selection.objects == [drawn])
    );

    let field = publish(&table, OWNER, &[(object(1), drawn), (object(7), source)]);
    let selection = select(&field.actions()[0]);
    let target = SeenObject {
        id: field.actions()[0].reference.clone(),
        zone: FIELD,
    };
    let arrow = Action::Arrow {
        selection: select(&field.actions()[1]),
        to: Target::Object(target),
    };
    let tap_and_aim = Action::Batch {
        actions: vec![
            Action::Tap {
                selection,
                tapped: true,
            },
            arrow,
        ],
    };
    field
        .apply(&mut table, &mut log, OWNER, tap_and_aim, &mut chance)
        .unwrap();
    assert!(table.objects[&drawn].tapped);
    assert!(!table.objects[&source].tapped);
    assert_eq!(table.arrows[0].from, source);
    assert_eq!(table.arrows[0].to, ArrowTarget::Object(drawn));
}

#[test]
fn lending_allows_a_guest_to_act_on_an_anonymous_card_until_shuffle() {
    let mut table = Table::new(2);
    let top = put(&mut table, 1000, LIBRARY);
    put(&mut table, 2000, LIBRARY);
    let mut log = Log::new(&table);
    let mut chance = Chance::fresh(7);
    let objects = [(object(5), top)];
    assert!(publish(&table, GUEST, &objects).actions().is_empty());
    let owner = publish(&table, OWNER, &objects);
    owner
        .apply(
            &mut table,
            &mut log,
            OWNER,
            Action::Lend {
                zone: LIBRARY,
                to: GUEST,
            },
            &mut chance,
        )
        .unwrap();

    let owner = publish(&table, OWNER, &objects);
    let guest = publish(&table, GUEST, &objects);
    let graveyard = Destination::Zone(Zone::Graveyard(OWNER));
    assert_eq!(
        guest.apply(
            &mut table,
            &mut log,
            GUEST,
            move_to(&owner.actions()[0], graveyard),
            &mut chance
        ),
        Err(Refusal::Stale)
    );
    let stale = guest.actions()[0].clone();
    assert!(!table.objects[&top].known_to.contains(&GUEST));
    guest
        .apply(
            &mut table,
            &mut log,
            GUEST,
            move_to(&stale, graveyard),
            &mut chance,
        )
        .unwrap();
    assert_eq!(table.zone(Zone::Graveyard(OWNER)).unwrap(), [top]);

    let old_top = table.zone(LIBRARY).unwrap()[0];
    let guest = publish(&table, GUEST, &[(object(5), old_top)]);
    let pre_shuffle = guest.actions()[0].clone();
    let owner = publish(&table, OWNER, &[(object(5), old_top)]);
    let old_owner = owner.actions()[0].clone();
    owner
        .apply(
            &mut table,
            &mut log,
            OWNER,
            Action::Shuffle {
                seat: OWNER,
                portion: Portion::All,
            },
            &mut chance,
        )
        .unwrap();
    assert_eq!(
        guest.apply(
            &mut table,
            &mut log,
            GUEST,
            move_to(&pre_shuffle, graveyard),
            &mut chance
        ),
        Err(Refusal::Stale)
    );
    let new_top = table.zone(LIBRARY).unwrap()[0];
    assert_ne!(new_top, old_top);
    let replacement = [(object(5), new_top)];
    assert!(publish(&table, GUEST, &replacement).actions().is_empty());
    let owner = publish(&table, OWNER, &replacement);
    assert_eq!(
        owner.apply(
            &mut table,
            &mut log,
            OWNER,
            move_to(&old_owner, graveyard),
            &mut chance
        ),
        Err(Refusal::Stale)
    );
    owner
        .apply(
            &mut table,
            &mut log,
            OWNER,
            move_to(&owner.actions()[0], graveyard),
            &mut chance,
        )
        .unwrap();
    assert_eq!(table.zone(Zone::Graveyard(OWNER)).unwrap(), [new_top, top]);
}

#[test]
fn a_reference_cannot_be_used_with_a_different_zone() {
    let mut table = Table::new(1);
    let id = put(&mut table, 1000, LIBRARY);
    let mut log = Log::new(&table);
    let mut chance = Chance::fresh(42);
    let owner = publish(&table, OWNER, &[(object(5), id)]);
    let mut wrong_zone = select(&owner.actions()[0]);
    wrong_zone.zone = HAND;
    assert_eq!(
        owner.apply(
            &mut table,
            &mut log,
            OWNER,
            Action::Remove {
                selection: wrong_zone
            },
            &mut chance
        ),
        Err(Refusal::Stale)
    );
}

#[test]
fn two_identical_stack_markers_get_distinct_action_references() {
    let mut table = Table::new(2);
    let source = put(&mut table, 70_000, FIELD);
    let mut log = Log::new(&table);
    let mut chance = Chance::fresh(42);
    let source_view = publish(&table, OWNER, &[(object(1), source)]);
    let marker = Action::Marker {
        selection: select(&source_view.actions()[0]),
        text: "Same ability".into(),
    };
    source_view
        .apply(
            &mut table,
            &mut log,
            OWNER,
            Action::Batch {
                actions: vec![marker.clone(), marker],
            },
            &mut chance,
        )
        .unwrap();
    let [first, second] = table.stack[..] else {
        panic!("expected two markers");
    };
    assert_eq!(table.markers[&first], table.markers[&second]);
    let subjects = [
        (Subject::Ability { id: first.0 }, first),
        (Subject::Ability { id: second.0 }, second),
    ];
    let guest = publish(&table, GUEST, &subjects);
    guest
        .apply(
            &mut table,
            &mut log,
            GUEST,
            move_to(
                &guest.actions()[1],
                Destination::Computed(ComputedDestination::Resolve),
            ),
            &mut chance,
        )
        .unwrap();
    assert_eq!(table.stack, [first]);
}
