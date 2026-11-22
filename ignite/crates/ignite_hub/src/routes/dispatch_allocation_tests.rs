//! Allocation through actual hand-offs, where both claims and reservations count.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::fleet::{Allocations, FleetSettings};
use ignite_core::ids::TicketId;
use ignite_core::shares::Source;
use ignite_store::hub::{self, batch, fleet_settings};

use super::tests::{at, claim_reserved, free, landed, post, register, start};

fn write(conn: &mut Connection, ops: &Value) -> Vec<TicketId> {
    let body = serde_json::from_value(json!({"ops": ops})).unwrap();
    let batch::BatchOutcome::Applied(reply) =
        batch::apply(conn, &body, "test", "", start()).unwrap()
    else {
        panic!("batch conflicted");
    };
    reply.results.into_iter().map(|result| result.id).collect()
}

fn ask(conn: &mut Connection, n: u32) -> (String, Value) {
    let session = format!("local_{n}");
    let (status, ticket) = post(conn, "/next", &free("m-1", &[&session]), at(f64::from(n)));
    assert_eq!(status, 200);
    (session, ticket)
}

#[test]
fn six_workers_balance_at_four_and_two_and_refill_the_pool_that_finished() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let mut ops = vec![
        json!({"op":"create","fields":{"type":"epic","title":"Wide","allocation":200}}),
        json!({"op":"create","fields":{"type":"epic","title":"Narrow","allocation":100}}),
        json!({"op":"create","parent":"$0","fields":{"type":"milestone","title":"Inherits"}}),
    ];
    // The newest ticket in the narrow pool outranks its siblings but cannot jump ahead of the
    // wide pool. The milestone has no local allocation and shares the wide pool's workers.
    for _ in 0..6 {
        ops.push(json!({"op":"create","parent":"$2","fields":{"title":"Wide ticket"}}));
        ops.push(json!({"op":"create","parent":"$1","fields":{"title":"Narrow ticket"}}));
    }
    ops.push(json!({"op":"create","parent":"$1","fields":{"title":"First narrow ticket","allocation":900}}));
    let ids = write(&mut conn, &json!(ops));
    conn.execute("UPDATE tickets SET cores = 0.1 WHERE type = 'task'", [])
        .unwrap();
    let wide = format!("{}.", ids[0]);
    let narrow = format!("{}.", ids[1]);
    let mut handoffs = Vec::new();
    for n in 1..=6 {
        let (session, reservation) = ask(&mut conn, n);
        let id = reservation["ticket"].as_str().unwrap().to_owned();
        if n % 2 == 0 {
            let (status, response) = post(
                &mut conn,
                "/claims",
                &claim_reserved(&reservation, &session),
                at(f64::from(n)),
            );
            assert_eq!(status, 200, "{response}");
        }
        handoffs.push((session, id));
    }
    assert!(handoffs[0].1.starts_with(&wide));
    assert_eq!(handoffs[1].1, ids.last().unwrap().as_str());
    assert_eq!(
        handoffs
            .iter()
            .filter(|(_, id)| id.starts_with(&wide))
            .count(),
        4
    );
    assert_eq!(
        handoffs
            .iter()
            .filter(|(_, id)| id.starts_with(&narrow))
            .count(),
        2
    );
    let (session, id) = &handoffs[1];
    let (status, body) = post(&mut conn, "/landed", &landed(id, session, "Done"), at(7.0));
    assert_eq!(status, 200, "{body}");
    let (_, replacement) = ask(&mut conn, 8);
    assert!(replacement["ticket"].as_str().unwrap().starts_with(&narrow));
}

#[test]
fn source_pool_and_general_pool_can_be_parked_without_stopping_a_regression() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = write(
        &mut conn,
        &json!([
            {"op":"create","fields":{"title":"General work"}},
            {"op":"create","fields":{"title":"Flaky check","labels":["flaky","hub"]}},
            {"op":"create","fields":{"title":"Parked ticket","allocation":0}},
            {"op":"create","fields":{"title":"Broken tip","allocation":0}},
            {"op":"create","fields":{"title":"Pending flaky check","status":"pending_approval","labels":["flaky","hub"]}}
        ]),
    );
    let set = |conn: &mut Connection, version, general, flaky| {
        let settings = FleetSettings {
            allocations: Some(Allocations {
                general: Some(general),
                sources: BTreeMap::from([(Source::Flaky, flaky)]),
            }),
            ..FleetSettings::default()
        };
        assert!(matches!(
            fleet_settings::put(conn, version, settings, "test", start()).unwrap(),
            fleet_settings::PutOutcome::Stored(_)
        ));
    };
    set(&mut conn, 0, 0, 0);
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('test:failed', ?1, 'bad', 'good', 'open')",
        [ids[3].as_str()],
    )
    .unwrap();
    assert_eq!(ask(&mut conn, 1).1["ticket"], ids[3].as_str());
    assert_eq!(ask(&mut conn, 2).1, Value::Null);
    set(&mut conn, 1, 0, 200);
    assert_eq!(ask(&mut conn, 3).1["ticket"], ids[1].as_str());
    assert_eq!(ask(&mut conn, 4).1, Value::Null);
    set(&mut conn, 2, 100, 200);
    assert_eq!(ask(&mut conn, 5).1["ticket"], ids[0].as_str());
    assert_eq!(ask(&mut conn, 6).1, Value::Null);
}

#[test]
fn a_database_from_before_allocation_keeps_its_eligible_and_parked_work() {
    let mut conn = Connection::open_in_memory().unwrap();
    let migrations = hub::MIGRATIONS;
    let before_allocation = migrations
        .iter()
        .position(|migration| migration.version == 46)
        .unwrap();
    ignite_store::migrate::apply(&mut conn, &migrations[..before_allocation]).unwrap();
    conn.execute_batch(
        "INSERT INTO tickets (id, parent, type, title, workers, status, priority,
                              assignee, created_at, updated_at, created_by, version)
         VALUES
           ('E100', NULL, 'epic', 'Wide', 6, 'open', 2, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 1),
           ('E100.1', 'E100', 'milestone', 'Inherited', NULL, 'open', 2, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 2),
           ('E100.1.1', 'E100.1', 'task', 'Reserved', NULL, 'open', 2, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 3),
           ('E100.1.2', 'E100.1', 'task', 'Blocked on reservation', NULL, 'open', 2, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 4),
           ('E200', NULL, 'epic', 'Narrow', 3, 'open', 2, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 5),
           ('E200.1', 'E200', 'task', 'Claimed', NULL, 'in_progress', 2, 'local_busy', '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 6),
           ('E200.2', 'E200', 'task', 'Low', NULL, 'open', 3, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 7),
           ('E200.3', 'E200', 'task', 'High', NULL, 'open', 1, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 8),
           ('E300', NULL, 'epic', 'Parked pool', 0, 'open', 2, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 9),
           ('E300.1', 'E300', 'task', 'Parked by pool', NULL, 'open', 0, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 10),
           ('T100', NULL, 'task', 'Pending', NULL, 'pending_approval', 0, NULL, '2026-11-17T12:00:00Z', '2026-11-17T12:00:00Z', 'seed', 11);
         INSERT INTO reservations (ticket, machine, session, expires_at, code)
           VALUES ('E100.1.1', 'm-1', 'local_reserved', '2026-11-17T14:05:00Z', '00000000');
         INSERT INTO edges (src, dst) VALUES ('E100.1.2', 'E100.1.1');
         INSERT INTO history (ticket, seq, actor, at, change)
           VALUES ('E200.1', 6, 'seed', '2026-11-17T12:00:00Z', '{}');",
    ).unwrap();
    ignite_store::migrate::apply(&mut conn, migrations).unwrap();

    let load = |id| hub::tickets::load(&conn, id).unwrap().unwrap();
    assert_eq!(load("E100").allocation, Some(6));
    assert_eq!(load("E200").allocation, Some(3));
    assert_eq!(load("E100.1").allocation, None);
    assert_eq!(load("E200.1").assignee.as_deref(), Some("local_busy"));
    assert_eq!(load("E300").allocation, Some(0));
    assert_eq!(
        load("T100").status,
        ignite_core::protocol::tickets::Status::PendingApproval
    );
    assert_eq!(
        conn.query_row(
            "SELECT seq FROM history WHERE ticket = 'E200.1'",
            [],
            |row| row.get::<_, u32>(0)
        )
        .unwrap(),
        6
    );

    assert_eq!(ask(&mut conn, 1).1["ticket"], "E200.3");
    assert_eq!(ask(&mut conn, 2).1["ticket"], "E200.2");
    assert_eq!(ask(&mut conn, 3).1, Value::Null);
}
