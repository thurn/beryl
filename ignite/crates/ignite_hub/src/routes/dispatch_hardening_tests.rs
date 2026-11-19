//! Which hub-filed tickets are offered and in what order, retried requests after a lost reply,
//! caps inherited from outer epics, and a retried close finishing what a first one left.

use serde_json::{Value, json};

use ignite_core::protocol::tickets::{BatchRequest, Operation, Status, TicketFields, TicketType};
use ignite_store::hub::{self, batch, findings, rows, tickets};

use super::tests::{
    Parent, at, claim, claim_reserved, file, file_under, free, post, register, start,
};

#[test]
fn every_label_the_hub_files_with_is_offered_under_an_inactive_epic() {
    for label in [
        "hub",
        "incomplete",
        "cache-audit",
        "graph-repair",
        "fleet",
        "flaky",
        "justiciar",
    ] {
        let mut conn = hub::open_in_memory().unwrap();
        let (_, ids) = file_under(&mut conn, Parent::Epic(None), &[("fault", &[label], &[])]);
        let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
        assert_eq!(reply["ticket"], ids[0].as_str(), "label {label}");
    }
    // A ticket with no such label under an inactive epic is still not offered, and `needs:` still
    // applies.
    let mut conn = hub::open_in_memory().unwrap();
    file_under(
        &mut conn,
        Parent::Epic(None),
        &[
            ("plain", &[], &[]),
            ("for a person", &["hub", "needs:person"], &[]),
        ],
    );
    let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reply, Value::Null);
}

#[test]
fn a_justiciar_ticket_is_offered_before_a_regression() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[
            ("fix a check", &[], &[]),
            ("the system is broken", &["justiciar"], &[]),
        ],
    );
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('rust:a::b', ?1, 'b1', 'a1', 'open')",
        [ids[0].as_str()],
    )
    .unwrap();
    let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reply["ticket"], ids[1].as_str());
}

#[test]
fn a_claim_retried_after_a_lost_reply_gets_the_same_brief() {
    let mut conn = hub::open_in_memory().unwrap();
    file(&mut conn, &[("work", &[], &[])]);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    let body = claim_reserved(&reservation, "local_a");
    let (status, first) = post(&mut conn, "/claims", &body, at(1.0));
    assert_eq!(status, 200, "{first}");
    let (status, again) = post(&mut conn, "/claims", &body, at(2.0));
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["ticket"]["assignee"], "local_a");
    assert_eq!(again["ticket"]["id"], first["ticket"]["id"]);
    // Another session still cannot take it.
    let other = claim(first["ticket"]["id"].as_str().unwrap(), "local_b");
    assert_eq!(post(&mut conn, "/claims", &other, at(3.0)).0, 409);
}

#[test]
fn a_cap_on_an_outer_epic_limits_the_epics_inside_it() {
    let mut conn = hub::open_in_memory().unwrap();
    let epic = |title: &str, parent: Option<&str>, workers, max_workers| Operation::Create {
        fields: TicketFields {
            title: Some(title.to_owned()),
            ticket_type: Some(TicketType::Epic),
            workers: Some(workers),
            max_workers: Some(max_workers),
            ..TicketFields::default()
        },
        parent: parent.map(str::to_owned),
        deps: Vec::new(),
        request_key: None,
    };
    let task = |title: &str| Operation::Create {
        fields: TicketFields {
            title: Some(title.to_owned()),
            ..TicketFields::default()
        },
        parent: Some("$1".to_owned()),
        deps: Vec::new(),
        request_key: None,
    };
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![
            epic("outer", None, None, Some(1)),
            epic("inner", Some("$0"), Some(3), None),
            task("one"),
            task("two"),
        ],
    };
    assert!(matches!(
        batch::apply(&mut conn, &request, "planner", "", start()).unwrap(),
        batch::BatchOutcome::Applied(_)
    ));
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert!(first["ticket"].is_string());
    let (_, second) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(1.0));
    assert_eq!(second, Value::Null);
}

#[test]
fn a_close_resent_after_a_batch_closed_the_ticket_frees_the_worker_and_the_finding() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let id = file(&mut conn, &[("improve", &[], &[])]).remove(0);
    let id = id.as_str();
    post(&mut conn, "/claims", &claim(id, "local_a"), start());
    let beat = json!({"machine": "m-1", "version": "d97ed0e", "workers": [
        {"session": "local_a", "app": "claude", "role": "worker", "state": "working",
         "ticket": id}]});
    assert_eq!(
        post(&mut conn, "/machines/heartbeat", &beat, at(1.0)).0,
        200
    );
    findings::upsert(
        &conn,
        &rows::Finding {
            rule: "slow-verb".to_owned(),
            subject: "check".to_owned(),
            unit: "seconds".to_owned(),
            cost: 3.0,
            saving: 2.0,
            ticket: Some(id.to_owned()),
            closed_at: None,
        },
    )
    .unwrap();
    // The ticket closed through a batch, as `ignite ticket close` does, and the close route's
    // later steps never ran.
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Close {
            id: id.to_owned(),
            reason: "done".to_owned(),
            include_ready: false,
        }],
    };
    batch::apply(&mut conn, &request, "local_a", "m-1", at(2.0)).unwrap();
    let close = json!({"ticket": id, "machine": "m-1", "session": "local_a", "reason": "done"});
    assert_eq!(post(&mut conn, "/close", &close, at(3.0)).0, 200);
    assert_eq!(
        tickets::load(&conn, id).unwrap().unwrap().status,
        Status::Closed
    );
    let state: String = conn
        .query_row(
            "SELECT state FROM workers WHERE session = 'local_a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "idle");
    assert!(findings::open(&conn).unwrap().is_empty());
}

/// Gives the ticket a saving, which makes it an improvement.
fn make_improvement(conn: &rusqlite::Connection, id: &str) {
    conn.execute(
        "UPDATE tickets SET saving = ?2 WHERE id = ?1",
        [
            id,
            r#"{"rule":"slow-verb","subject":"check","unit":"dollars","amount":5.0}"#,
        ],
    )
    .unwrap();
}

#[test]
fn an_improvement_respects_its_epic_s_cap() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let (epic, ids) = file_under(
        &mut conn,
        Parent::Epic(Some(2)),
        &[("work", &[], &[]), ("improve", &[], &[])],
    );
    make_improvement(&conn, ids[1].as_str());
    conn.execute(
        "UPDATE tickets SET max_workers = 1 WHERE id = ?1",
        [epic.unwrap().as_str()],
    )
    .unwrap();
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim(ids[0].as_str(), "local_a"),
        start(),
    );
    assert_eq!(status, 200);
    // The epic's one worker is taken, so its improvement waits like any other ticket.
    let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(1.0));
    assert_eq!(reply, Value::Null);
}

#[test]
fn a_lapsed_improvement_does_not_count_toward_the_improvement_share() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(&mut conn, &[("work", &[], &[]), ("improve", &[], &[])]);
    make_improvement(&conn, ids[1].as_str());
    // Lower priority, so that the improvement goes first only while improvements are due.
    conn.execute(
        "UPDATE tickets SET priority = 3 WHERE id = ?1",
        [ids[1].as_str()],
    )
    .unwrap();
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(first["ticket"], ids[1].as_str());
    // The reservation lapses unclaimed, so improvements are still under their share.
    let (_, again) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(3_600.0));
    assert_eq!(again["ticket"], ids[1].as_str());
    // Once claimed, the hand-off counts, and the improvement has its share.
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&again, "local_b"),
        at(3_601.0),
    );
    assert_eq!(status, 200);
    let (_, next) = post(&mut conn, "/next", &free("m-1", &["local_c"]), at(3_602.0));
    assert_eq!(next["ticket"], ids[0].as_str());
}
