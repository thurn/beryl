//! Which hub-filed tickets are offered and in what order, retried requests after a lost reply,
//! caps inherited from outer epics, and a retried close finishing what a first one left.

use serde_json::{Value, json};

use ignite_core::protocol::tickets::{
    BatchRequest, Operation, Saving, Status, TicketFields, TicketType,
};
use ignite_store::hub::{batch, findings, rows, tickets};

use super::tests::{
    Parent, at, claim, claim_reserved, file, file_under, free, open, post, register, start,
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
        let mut conn = open();
        let (_, ids) = file_under(&mut conn, Parent::Epic(None), &[("fault", &[label], &[])]);
        let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
        assert_eq!(reply["ticket"], ids[0].as_str(), "label {label}");
    }
    // A ticket with no such label under an inactive epic is still not offered, and `needs:` still
    // applies.
    let mut conn = open();
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
fn a_scheduled_ticket_s_copy_is_offered_and_an_allocation_of_zero_parks_it() {
    let mut conn = open();
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Create {
            fields: TicketFields {
                title: Some("Sweep stale tickets".to_owned()),
                ticket_type: Some(TicketType::Meta),
                every_hours: Some(Some(6)),
                ..TicketFields::default()
            },
            parent: None,
            deps: Vec::new(),
            request_key: None,
        }],
    };
    // Files the meta-ticket and its first copy, and returns both.
    let schedule = |conn: &mut rusqlite::Connection| {
        let batch::BatchOutcome::Applied(filed) =
            batch::apply(conn, &request, "person", "", start()).unwrap()
        else {
            panic!("the meta-ticket was refused");
        };
        let copies = crate::upkeep::schedules::file_due(conn, start()).unwrap();
        (filed.results[0].id.clone(), copies)
    };
    let (meta, copies) = schedule(&mut conn);
    assert_eq!(copies, vec![format!("{meta}.1")]);
    // The copy has a parent, the meta-ticket, which nobody gave an allocation.
    let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reply["ticket"], copies[0].as_str());

    // A person can still park the schedule's work, as with an epic.
    let mut conn = open();
    let (meta, _) = schedule(&mut conn);
    super::tests::set_workers(&mut conn, &meta, 0);
    let (_, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reply, Value::Null);
}

#[test]
fn a_justiciar_ticket_is_offered_before_a_regression() {
    let mut conn = open();
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
    let mut conn = open();
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
    let mut conn = open();
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
    let mut conn = open();
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
    let mut conn = open();
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
    let mut conn = open();
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

#[test]
fn a_ticket_filed_with_an_estimated_saving_is_handed_off_as_an_improvement() {
    let mut conn = open();
    let work = file(&mut conn, &[("work", &[], &[])]);
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Create {
            fields: TicketFields {
                title: Some("Shorten the gate's wait".to_owned()),
                // Under no epic and of lower priority than the epic's work, so only the
                // improvement share can put it first.
                priority: Some(3),
                saving: Some(Saving {
                    rule: Saving::ESTIMATE.to_owned(),
                    subject: "Shorten the gate's wait".to_owned(),
                    unit: "agent-hours".to_owned(),
                    amount: 12.5,
                }),
                ..TicketFields::default()
            },
            parent: None,
            deps: Vec::new(),
            request_key: None,
        }],
    };
    let batch::BatchOutcome::Applied(filed) =
        batch::apply(&mut conn, &request, "patrol", "", start()).unwrap()
    else {
        panic!("the create was refused");
    };
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(first["ticket"], filed.results[0].id.as_str());
    let (_, second) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(1.0));
    assert_eq!(second["ticket"], work[0].as_str());
}

#[test]
fn a_machine_whose_scheduler_is_full_is_offered_what_its_held_tickets_leave_room_for() {
    // A landing fills the scheduler, so it has no cores free at this moment, but the machine
    // admits six cores and each ticket expects three. The expected use fills the core budget
    // after two tickets, before the concurrent build limit of three.
    let mut conn = open();
    let ids = file(
        &mut conn,
        &[("one", &[], &[]), ("two", &[], &[]), ("three", &[], &[])],
    );
    conn.execute("UPDATE tickets SET cores = 3.0 WHERE type = 'task'", [])
        .unwrap();
    let ask = |conn: &mut rusqlite::Connection, session: &str| {
        let mut body = free("m-1", &[session]);
        body["free_cores"] = json!(0.0);
        body["admit_cores"] = json!(6.0);
        post(conn, "/next", &body, start()).1
    };
    let first = ask(&mut conn, "local_a");
    assert_eq!(first["ticket"], ids[0].as_str());
    // The first ticket is claimed and in progress; the second fits beside it.
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&first, "local_a"),
        start(),
    );
    assert_eq!(status, 200);
    assert_eq!(ask(&mut conn, "local_b")["ticket"], ids[1].as_str());
    // One in progress and one reserved account for all six cores.
    assert_eq!(ask(&mut conn, "local_c"), Value::Null);
}

/// `POST /next` holds the database's write lock for its whole transaction, so it must not read
/// the `metrics` table, which holds gigabytes of efficiency rows. When it summed last week's
/// dollars there, a slow scan held the lock for minutes and every other write failed with
/// `database is locked`. With the table renamed away, any read of it fails the request.
#[test]
fn a_hand_off_reads_no_metrics_rows() {
    let mut conn = open();
    let ids = file(
        &mut conn,
        &[("plain", &[], &[]), ("saving", &["improvement"], &[])],
    );
    crate::rules::backlog::refresh(&conn, start()).unwrap();
    conn.execute("ALTER TABLE metrics RENAME TO metrics_away", [])
        .unwrap();
    let (status, reply) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(status, 200, "{reply}");
    assert!(
        ids.iter().any(|id| reply["ticket"] == id.as_str()),
        "{reply}"
    );
}
