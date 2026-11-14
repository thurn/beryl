use beryl_table::change::{Edit, Location, ObjectEdit};
use beryl_table::state::{
    AbilityMarker, Arrow, ArrowKind, ArrowTarget, Card, Face, Object, ObjectId, Row, Zone,
};
use beryl_table::{Chance, Change, Refusal, Seat, Table};

const CARD: ObjectId = ObjectId(1);
const OTHER: ObjectId = ObjectId(2);
const OWNER: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn edit<T>(before: T, after: T) -> Edit<T> {
    Edit { before, after }
}

fn object(id: ObjectId) -> Object {
    Object::new(
        id,
        Card::Named("example card".into()),
        OWNER,
        Zone::Battlefield,
    )
}

fn assert_inverse(before: &Table, change: &Change) {
    let mut table = before.clone();
    change.perform(&mut table);
    change.inverse().perform(&mut table);
    assert_eq!(table, *before, "inverse of {change:?}");
}

#[test]
fn edits_restore_each_object_field() {
    let mut table = Table::new(2);
    Change::Create {
        object: object(CARD),
        index: 0,
    }
    .perform(&mut table);

    for object_edit in [
        ObjectEdit::Controller(edit(OWNER, GUEST)),
        ObjectEdit::Face(edit(Face::Front, Face::Down)),
        ObjectEdit::Tapped(edit(false, true)),
        ObjectEdit::Counters(edit(vec![], vec![("charge".into(), 2)])),
        ObjectEdit::Stats(edit(None, Some((3, 4)))),
        ObjectEdit::Damage(edit(0, 1)),
        ObjectEdit::Note(edit(None, Some("example note".into()))),
        ObjectEdit::AttachedTo(edit(None, Some(OTHER))),
        ObjectEdit::DoesntUntap(edit(false, true)),
        ObjectEdit::Row(edit(Row::Auto, Row::Lands)),
    ] {
        let change = Change::SetObject {
            id: CARD,
            edit: object_edit,
        };
        let mut after = table.clone();
        change.perform(&mut after);
        assert_ne!(table, after, "effect of {change:?}");
        assert_inverse(&table, &change);
    }
}

#[test]
fn inverses_restore_positions_ids_cards_markers_and_arrows() {
    let mut table = Table::new(2);
    let card = Change::Create {
        object: object(CARD),
        index: 0,
    };
    let marker = Change::CreateMarker {
        id: OTHER,
        marker: AbilityMarker {
            source: CARD,
            text: "example ability".into(),
        },
        index: 0,
    };
    let arrow = Change::InsertArrow {
        arrow: Arrow {
            from: CARD,
            to: ArrowTarget::Player(GUEST),
            by: OWNER,
            kind: ArrowKind::Attack,
        },
        index: 0,
    };
    for change in [card, marker, arrow] {
        assert_inverse(&table, &change);
        change.perform(&mut table);
        assert_inverse(&table, &change.inverse());
    }

    assert_inverse(
        &table,
        &Change::Move {
            id: CARD,
            from: Location {
                zone: Zone::Battlefield,
                index: 0,
            },
            to: Location {
                zone: Zone::Stack,
                index: 0,
            },
        },
    );
    assert_inverse(
        &table,
        &Change::ReplaceId {
            from: CARD,
            to: ObjectId(3),
        },
    );
    assert_inverse(
        &table,
        &Change::Random {
            outcomes: vec![0, 4],
        },
    );
    assert_inverse(&table, &Change::Nothing);
}

#[test]
fn undo_restores_forgotten_knowledge_and_keeps_anything_learned() {
    let mut table = Table::new(2);
    let mut seen = object(CARD);
    seen.known_to = vec![OWNER];
    Change::Create {
        object: seen,
        index: 0,
    }
    .perform(&mut table);

    let shuffle = Change::ResetKnowledge {
        id: CARD,
        edit: edit(vec![OWNER], vec![]),
        forget: true,
    };
    assert_inverse(&table, &shuffle);
    shuffle.perform(&mut table);
    let look = Change::Learn {
        id: CARD,
        seat: GUEST,
    };
    look.perform(&mut table);
    assert_eq!(look.inverse(), Change::Nothing);
    look.inverse().perform(&mut table);
    assert_eq!(table.objects[&CARD].known_to, [GUEST]);

    let inverse_shuffle = shuffle.inverse();
    inverse_shuffle.perform(&mut table);
    assert_eq!(table.objects[&CARD].known_to, [OWNER, GUEST]);
    inverse_shuffle.inverse().perform(&mut table);
    assert!(table.objects[&CARD].known_to.contains(&OWNER));
    assert!(table.objects[&CARD].known_to.contains(&GUEST));
}

#[test]
fn chance_replays_in_order_and_checks_each_requested_range() {
    let bounds = [4, 60, 2, 17];
    let mut fresh = Chance::fresh(0x1234_5678);
    let recorded: Vec<_> = bounds
        .iter()
        .map(|&upper| fresh.below(upper).unwrap())
        .collect();
    let mut replay = Chance::replaying(recorded.clone());
    for (&expected, upper) in recorded.iter().zip(bounds) {
        assert!(expected < upper);
        assert_eq!(replay.below(upper), Ok(expected));
    }
    assert_eq!(replay.below(1), Err(Refusal::Invalid));
    let mut replay = Chance::replaying(vec![4]);
    assert_eq!(replay.below(0), Err(Refusal::Invalid));
    assert_eq!(replay.below(4), Err(Refusal::Invalid));
    assert_eq!(replay.below(5), Ok(4));
}

#[test]
fn inverses_restore_lends_and_library_top_settings() {
    let mut table = Table::new(2);
    for change in [
        Change::Lend {
            zone: Zone::Library(OWNER),
            to: GUEST,
            index: 0,
            on: true,
        },
        Change::AlwaysRevealTop {
            seat: OWNER,
            index: 0,
            on: true,
        },
        Change::AlwaysLookAtTop {
            seat: GUEST,
            index: 0,
            on: true,
        },
    ] {
        assert_inverse(&table, &change);
        change.perform(&mut table);
        assert_inverse(&table, &change.inverse());
    }
}
