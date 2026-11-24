use beryl_protocol::protocol::TableAction;
use beryl_protocol::script::{Table as Stage, failure};
use beryl_table::action::{Destination, Position, Selection};
use beryl_table::state::{Card, Object, ObjectId, Zone};
use beryl_table::{Action, Chance, Change, Log, Refusal, Seat, Table};
use beryl_table_session::publication::PublishedTable;

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn add(table: &mut Table, id: u32, owner: Seat, name: &str, zone: Zone, known: &[Seat]) {
    let mut object = Object::new(ObjectId(id), Card::Named(name.into()), owner, zone);
    object.known_to = known.to_vec();
    Change::Create { object, index: 0 }.perform(table);
}

#[test]
fn checked_client_actions_publish_both_seats_with_fresh_private_mappings() {
    let mut table = Table::new(2);
    add(
        &mut table,
        1,
        HOST,
        "Island",
        Zone::Battlefield,
        &[HOST, GUEST],
    );
    add(&mut table, 2, HOST, "Ponder", Zone::Library(HOST), &[]);
    add(&mut table, 3, GUEST, "Opt", Zone::Hand(GUEST), &[GUEST]);
    let log = Log::new(&table);
    let mut table = PublishedTable::new(table, log, Chance::fresh(11)).unwrap();

    let move_public_card = |table: &PublishedTable, viewer| -> TableAction {
        let view = table.publication(viewer).unwrap().view();
        let reference = view
            .table
            .as_ref()
            .unwrap()
            .object_actions
            .iter()
            .find(|action| action.zone == Zone::Battlefield)
            .unwrap()
            .reference
            .clone();
        Action::Move {
            selection: Selection {
                objects: vec![reference],
                zone: Zone::Battlefield,
            },
            to: Destination::Zone(Zone::Graveyard(HOST)),
            position: Position::Top,
            face: None,
            tapped: None,
        }
    };
    let before = [HOST, GUEST].map(|seat| table.publication(seat).unwrap().view().clone());
    let old_actions = [HOST, GUEST].map(|seat| move_public_card(&table, seat));

    let entry = table
        .apply_client(
            HOST,
            Action::Draw {
                seat: HOST,
                count: 1,
            },
        )
        .unwrap();
    assert_eq!(entry.by, HOST);
    for (seat, before, hidden, known) in [
        (HOST, &before[0], "Opt", "Ponder"),
        (GUEST, &before[1], "Ponder", "Opt"),
    ] {
        let after = table.publication(seat).unwrap().view();
        let script = after.script.as_ref().unwrap();
        assert_eq!(failure(&Stage::of(before), script, after), None);
        let json = serde_json::to_string(after).unwrap();
        assert!(json.contains(known));
        assert!(!json.contains(hidden));
        assert_eq!(after.table.as_ref().unwrap().log.len(), 1);
    }

    let current_host = move_public_card(&table, HOST);
    let current_guest = move_public_card(&table, GUEST);
    assert_ne!(old_actions[0], current_host);
    assert_ne!(old_actions[1], current_guest);
    for (seat, old) in [HOST, GUEST].into_iter().zip(old_actions) {
        assert_eq!(table.apply_client(seat, old), Err(Refusal::Stale));
    }
    assert_eq!(table.apply_client(GUEST, current_host), Err(Refusal::Stale));
    table.apply_client(GUEST, current_guest).unwrap();
    assert_eq!(table.log().entries().len(), 2);
    assert_eq!(table.log().entries()[1].by, GUEST);
}
