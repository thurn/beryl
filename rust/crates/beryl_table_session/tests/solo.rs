use std::sync::mpsc::{Receiver, channel};

use beryl_protocol::ipc::{TableActReply, TableRefusalReason, TableRefused};
use beryl_protocol::protocol::{PlayerView, TableObjectRef};
use beryl_protocol::script::{Subject, Table as Stage, failure};
use beryl_protocol::{GameUpdate, SessionIds};
use beryl_session::Sessions;
use beryl_table::action::{Adjustment, ComputedDestination, Destination, Position, Selection};
use beryl_table::deck::parse_text;
use beryl_table::state::Zone;
use beryl_table::{Action, Refusal, Seat};
use beryl_table_session::{TableSessions, refusal};

const SOLO: Seat = Seat(0);

fn receiving_tables() -> (TableSessions, Receiver<GameUpdate>) {
    let (send, receive) = channel();
    let tables = TableSessions::new(SessionIds::default(), move |update| {
        send.send(update).unwrap();
    });
    (tables, receive)
}

fn reference(view: &PlayerView, subject: Subject) -> TableObjectRef {
    view.table
        .as_ref()
        .unwrap()
        .object_actions
        .iter()
        .find(|action| action.subject == subject)
        .unwrap()
        .reference
        .clone()
}

fn next_update(previous: &GameUpdate, receive: &Receiver<GameUpdate>) -> GameUpdate {
    let update = receive.try_recv().unwrap();
    assert_eq!(update.session, previous.session);
    let script = update.view.script.as_ref().unwrap();
    assert_eq!(
        failure(&Stage::of(&previous.view), script, &update.view),
        None
    );
    update
}

#[test]
fn start_draw_play_tap_and_undo_emit_updates() {
    let (tables, receive) = receiving_tables();
    let id = tables.solo(parse_text("12 Island\n\n2 Plains")).unwrap();
    let start = receive.try_recv().unwrap();
    let table = start.view.table.as_ref().unwrap();
    assert_eq!(start.session, id);
    assert_eq!(table.seats, 1);
    assert_eq!(table.orders.you.hand.len(), 7);
    assert_eq!(table.orders.you.sideboard.len(), 2);
    assert!(table.orders.opponent.hand.is_empty());
    assert_eq!(start.view.players.you.library.len(), 5);
    assert!(start.view.script.as_ref().unwrap().reset.is_some());

    tables
        .act(Action::Draw {
            seat: SOLO,
            count: 1,
        })
        .unwrap();
    let draw = next_update(&start, &receive);
    let hand = &draw.view.table.as_ref().unwrap().orders.you.hand;
    assert_eq!(hand.len(), 8);
    assert_eq!(draw.view.players.you.library.len(), 4);
    let slot = *hand.last().unwrap();
    let card = Subject::Object { slot };
    let move_card = Action::Move {
        selection: Selection {
            objects: vec![reference(&draw.view, card)],
            zone: Zone::Hand(SOLO),
        },
        to: Destination::Computed(ComputedDestination::Play),
        position: Position::Top,
        face: None,
        tapped: None,
    };
    tables.act(move_card.clone()).unwrap();
    let played = next_update(&draw, &receive);
    assert_eq!(played.view.battlefield, [slot]);
    assert_eq!(played.view.table.as_ref().unwrap().orders.you.hand.len(), 7);

    assert_eq!(tables.act(move_card), Err(Refusal::Stale));
    assert!(receive.try_recv().is_err());
    assert_eq!(tables.current().unwrap(), played);
    let tap = Action::Tap {
        selection: Selection {
            objects: vec![reference(&played.view, card)],
            zone: Zone::Battlefield,
        },
        tapped: true,
    };
    tables.act(tap).unwrap();
    let tapped = next_update(&played, &receive);
    assert!(
        tapped
            .view
            .objects
            .iter()
            .find(|o| o.slot == slot)
            .unwrap()
            .tapped
    );

    tables.act(Action::Undo { entry: None }).unwrap();
    let undone = next_update(&tapped, &receive);
    assert!(
        !undone
            .view
            .objects
            .iter()
            .find(|o| o.slot == slot)
            .unwrap()
            .tapped
    );
    assert_eq!(tables.current().unwrap(), undone);
}

#[test]
fn replacement_and_leave_retire_the_old_view() {
    let engine = Sessions::new(None, |_| {});
    let ids = engine.session_ids();
    let tables = TableSessions::new(ids.clone(), |_| {});
    assert!(tables.current().is_none());

    let before = ids.allocate();
    let first = tables.solo(parse_text("10 Island")).unwrap();
    assert!(first > before);
    let update = tables.current().unwrap();
    assert_eq!(tables.solo(parse_text("")), Err(Refusal::INVALID));
    assert_eq!(tables.current().unwrap(), update);

    let slot = update.view.table.as_ref().unwrap().orders.you.hand[0];
    let move_card = Action::Move {
        selection: Selection {
            objects: vec![reference(&update.view, Subject::Object { slot })],
            zone: Zone::Hand(SOLO),
        },
        to: Destination::Computed(ComputedDestination::Play),
        position: Position::Top,
        face: None,
        tapped: None,
    };
    let second = tables.solo(parse_text("10 Island")).unwrap();
    assert!(second > first);
    assert_eq!(tables.act(move_card), Err(Refusal::Stale));
    tables.leave();
    assert!(tables.current().is_none());
    assert_eq!(
        tables.act(Action::Undo { entry: None }),
        Err(Refusal::Stale)
    );
    assert!(ids.allocate() > second);
}

#[test]
fn a_refusal_preserves_blocking_entry_numbers_in_the_reply() {
    let (tables, updates) = receiving_tables();
    tables.solo_seeded(parse_text("10 Island"), 0).unwrap();
    updates.try_recv().unwrap();

    let life_edit = Action::Life {
        seat: SOLO,
        adjustment: Adjustment::Add(-1),
    };
    // The log offers undo by entry number for a batch.
    tables
        .act(Action::Batch {
            actions: vec![life_edit.clone()],
        })
        .unwrap();
    let batch = updates.try_recv().unwrap();
    let entry = batch.view.table.unwrap().log.last().unwrap().n;
    for _ in 0..2 {
        tables.act(life_edit.clone()).unwrap();
        updates.try_recv().unwrap();
    }

    let local = tables.act(Action::Undo { entry: Some(entry) });
    let blockers = vec![entry + 1, entry + 2];
    let wire = refusal::to_wire(local.clone().unwrap_err());
    let linked = refusal::reply_from_wire(wire);
    assert_eq!(linked, TableActReply::from(local));
    assert_eq!(
        linked,
        TableActReply::Refused(TableRefused {
            refused: TableRefusalReason::Invalid,
            blocked_by: Some(blockers.clone()),
        }),
    );
    assert_eq!(
        serde_json::to_value(linked).unwrap(),
        serde_json::json!({ "refused": "invalid", "blocked_by": blockers }),
    );
    assert!(updates.try_recv().is_err());
}
