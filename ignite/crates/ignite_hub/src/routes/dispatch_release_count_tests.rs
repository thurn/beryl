//! A ticket released by different sessions is held back from the pool and goes to a justiciar
//! (see `dispatch_releases.rs`), until its text changes or a person releases it again.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::ids::MachineId;
use ignite_core::protocol::hub::{
    FoundBy, RegressionReport, ReleaseCause, ResultKey, TipState, TipVerified,
};
use ignite_core::protocol::tickets::Status;
use ignite_core::time::Timestamp;
use ignite_store::hub::{releases, tickets, tips};

use super::tests::{at, claim, claim_reserved, file, free, landed, open, post, release};
use super::{Request, Response};
use crate::auth::Writer;
use crate::upkeep::{Upkeep, filing::UPKEEP, justiciar, regressions};

/// Claims `ticket` by name for `session` and releases it with `note` and `cause`.
fn work_and_release(
    conn: &mut Connection,
    ticket: &str,
    session: &str,
    note: &str,
    cause: Option<&str>,
    now: Timestamp,
) {
    let (status, reply) = post(conn, "/claims", &claim(ticket, session), now);
    assert_eq!(status, 200, "{reply}");
    let mut body = release(ticket, session);
    body["reason"] = json!(note);
    if let Some(cause) = cause {
        body["cause"] = json!(cause);
    }
    let (status, reply) = post(conn, "/release", &body, now.plus_seconds(1.0));
    assert_eq!(status, 200, "{reply}");
}

/// The ticket the pool would offer the idle worker `session` now, if any. Asking reserves it
/// for that worker, so each ask names a new one.
fn offered(conn: &mut Connection, session: &str, now: Timestamp) -> Option<String> {
    let (status, reply) = post(conn, "/next", &free("m-1", &[session]), now);
    assert_eq!(status, 200, "{reply}");
    reply["ticket"].as_str().map(str::to_owned)
}

/// Each justiciar ticket's ID and body, and how many comments it has.
fn justiciar_tickets(conn: &Connection) -> Vec<(String, String, i64)> {
    let mut statement = conn
        .prepare(
            "SELECT id, body, (SELECT count(*) FROM comments c WHERE c.ticket = t.id)
             FROM tickets t
             WHERE EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = 'justiciar')
             ORDER BY id",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// Sends a request to the ticket routes as `m-1`.
fn tickets_route(conn: &mut Connection, method: &str, path: &str, body: &Value) -> Response {
    let request = Request {
        method: method.into(),
        path: path.into(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: MachineId::new("m-1").unwrap(),
            actor: "planner".into(),
        }),
        body: serde_json::to_vec(body).unwrap(),
        headers: Vec::new(),
    };
    crate::routes::tickets::handle(conn, &request, at(100.0)).expect("a ticket route")
}

fn shown(conn: &mut Connection, ticket: &str) -> Value {
    let response = tickets_route(conn, "GET", &format!("/tickets/{ticket}"), &Value::Null);
    assert_eq!(response.status, 200);
    serde_json::from_slice(&response.body).unwrap()
}

fn update(conn: &mut Connection, ticket: &str, set: &Value) {
    let batch = json!({"ops": [{"op": "update", "id": ticket, "set": set}]});
    let response = tickets_route(conn, "POST", "/batch", &batch);
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
}

#[test]
fn two_sessions_releasing_a_ticket_hold_it_back_and_file_one_justiciar_ticket() {
    let mut conn = open();
    let id = file(&mut conn, &[("needs a fixture", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(
        &mut conn,
        id,
        "local_a",
        "The fixture is missing.",
        None,
        at(0.0),
    );
    // One session releasing twice is one worker's obstacle, not yet the ticket's.
    work_and_release(&mut conn, id, "local_a", "Still missing.", None, at(10.0));
    assert!(justiciar_tickets(&conn).is_empty());
    assert!(shown(&mut conn, id)["held_back"].is_null());
    work_and_release(
        &mut conn,
        id,
        "local_b",
        "No fixture here either.",
        Some("taken-back"),
        at(20.0),
    );

    let filed = justiciar_tickets(&conn);
    assert_eq!(filed.len(), 1, "{filed:?}");
    let (justiciar, body, comments) = &filed[0];
    assert!(justiciar.starts_with('J'), "{justiciar}");
    assert!(body.contains(id), "{body}");
    for note in ["> The fixture is missing.", "> No fixture here either."] {
        assert!(body.contains(note), "{body}");
    }
    assert_eq!(*comments, 0);
    let detail = shown(&mut conn, id);
    assert_eq!(detail["releases"]["count"], 3);
    assert!(
        detail["held_back"].as_str().unwrap().contains("2 sessions"),
        "{detail}"
    );
    // The justiciar ticket is offered; the held-back ticket is not.
    assert_eq!(
        offered(&mut conn, "local_idle", at(30.0)),
        Some(filed[0].0.clone())
    );
}

#[test]
fn a_third_session_comments_on_the_justiciar_ticket_instead_of_filing_another() {
    let mut conn = open();
    let id = file(&mut conn, &[("impossible", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "first", None, at(0.0));
    work_and_release(&mut conn, id, "local_b", "second", None, at(10.0));
    // A session can still start the held-back ticket by name, as a justiciar does.
    work_and_release(&mut conn, id, "local_c", "third", None, at(20.0));
    let filed = justiciar_tickets(&conn);
    assert_eq!(filed.len(), 1, "{filed:?}");
    assert_eq!(filed[0].2, 1, "the third release adds one comment");
}

#[test]
fn a_release_at_the_plan_limit_or_with_needs_is_not_counted() {
    let mut conn = open();
    let id = file(&mut conn, &[("ordinary", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "gave up", None, at(0.0));
    work_and_release(
        &mut conn,
        id,
        "local_b",
        "limit",
        Some("plan-limit"),
        at(10.0),
    );
    work_and_release(&mut conn, id, "local_c", "held", Some("cli-hold"), at(20.0));
    let (status, _) = post(&mut conn, "/claims", &claim(id, "local_d"), at(30.0));
    assert_eq!(status, 200);
    let mut body = release(id, "local_d");
    body["needs"] = json!("windows");
    assert_eq!(post(&mut conn, "/release", &body, at(31.0)).0, 200);
    assert!(justiciar_tickets(&conn).is_empty());
    assert_eq!(shown(&mut conn, id)["releases"]["count"], 1);
    update(&mut conn, id, &json!({"needs_os": null}));
    assert_eq!(
        offered(&mut conn, "local_idle", at(40.0)).as_deref(),
        Some(id)
    );
}

#[test]
fn a_ticket_deferred_before_its_release_is_not_held_back() {
    let mut conn = open();
    let id = file(&mut conn, &[("waits on a clock", &[], &[])]).remove(0);
    let id = id.as_str();
    for (session, start) in [("local_a", 0.0), ("local_b", 10.0)] {
        let (status, reply) = post(&mut conn, "/claims", &claim(id, session), at(start));
        assert_eq!(status, 200, "{reply}");
        let defer = json!({"ops": [{"op": "defer", "id": id, "until": at(start + 5.0)}]});
        assert_eq!(
            tickets_route(&mut conn, "POST", "/batch", &defer).status,
            200
        );
        let mut body = release(id, session);
        body["reason"] = json!("The earlier work is still running.");
        assert_eq!(post(&mut conn, "/release", &body, at(start + 1.0)).0, 200);
    }
    assert!(justiciar_tickets(&conn).is_empty());
    assert!(shown(&mut conn, id)["releases"].is_null());
    assert_eq!(
        offered(&mut conn, "local_idle", at(20.0)).as_deref(),
        Some(id)
    );
}

#[test]
fn editing_the_acceptance_offers_the_ticket_again() {
    let mut conn = open();
    let id = file(&mut conn, &[("vague", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "first", None, at(0.0));
    work_and_release(&mut conn, id, "local_b", "second", None, at(10.0));
    // Only the justiciar ticket is left to offer, and taking it leaves nothing.
    let justiciar = justiciar_tickets(&conn).remove(0).0;
    assert_eq!(offered(&mut conn, "local_x", at(20.0)), Some(justiciar));
    assert_eq!(offered(&mut conn, "local_y", at(21.0)), None);

    update(
        &mut conn,
        id,
        &json!({"acceptance": "`cargo test` passes with a test of X."}),
    );
    let detail = shown(&mut conn, id);
    assert!(detail["held_back"].is_null(), "{detail}");
    assert!(detail["releases"].is_null(), "{detail}");
    assert_eq!(offered(&mut conn, "local_z", at(22.0)).as_deref(), Some(id));
}

#[test]
fn releasing_the_ticket_again_offers_it_again() {
    let mut conn = open();
    let id = file(&mut conn, &[("blocked", &[], &[])]).remove(0);
    let id = id.as_str();
    work_and_release(&mut conn, id, "local_a", "first", None, at(0.0));
    work_and_release(&mut conn, id, "local_b", "second", None, at(10.0));
    // `ignite ticket update <id> --release` sends the status `open`.
    update(&mut conn, id, &json!({"status": "open"}));
    assert!(shown(&mut conn, id)["held_back"].is_null());
}

fn regression(conn: &mut Connection, check: &str, seconds: f64) -> String {
    let report = RegressionReport {
        check: check.parse().unwrap(),
        first_bad: "1111111".parse().unwrap(),
        last_good: "0000000".parse().unwrap(),
        output: "the check failed".to_owned(),
        caused_by: None,
        suspects: Vec::new(),
        platform: None,
        search: None,
        found_by: FoundBy::TipVerification,
        detected_at: None,
    };
    regressions::file(conn, UPKEEP, &report, at(seconds))
        .unwrap()
        .0
}

fn key(check: &str) -> ResultKey {
    ResultKey {
        check: check.parse().unwrap(),
        hash: "a".repeat(64).parse().unwrap(),
        platform: None,
    }
}

fn verified(commit: &str, checks: &[&str]) -> TipVerified {
    TipVerified {
        commit: commit.parse().unwrap(),
        machine: "m-1".parse().unwrap(),
        state: TipState::Verified,
        regression_passes: Some(checks.iter().map(|check| key(check)).collect()),
        failed_descendant: None,
        failure: None,
    }
}

fn verify(conn: &mut Connection, tip: &TipVerified, seconds: f64) {
    let request = Request {
        method: "POST".into(),
        path: "/tips/verified".into(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: tip.machine.clone(),
            actor: "verifier".into(),
        }),
        body: serde_json::to_vec(tip).unwrap(),
        headers: Vec::new(),
    };
    let response = crate::routes::checks::handle(conn, &request, at(seconds)).unwrap();
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
}

#[test]
fn workers_stop_getting_the_grouped_regression_when_the_last_check_passes() {
    let mut conn = open();
    let rust = [
        "rust:suite::one",
        "rust:suite::two",
        "rust:suite::three",
        "rust:suite::four",
        "rust:suite::five",
        "rust:suite::six",
    ];
    let boards = "frame-counts:boards";
    let id = regression(&mut conn, rust[0], 1.0);
    for check in rust.iter().skip(1).chain([&boards]) {
        assert_eq!(regression(&mut conn, check, 2.0), id);
    }

    // One worker gives up before the separate fixes land. The Rust fix alone is insufficient.
    work_and_release(
        &mut conn,
        &id,
        "local_a",
        "The board budget still fails.",
        None,
        at(3.0),
    );
    verify(&mut conn, &verified("2222222", &rust), 5.0);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(6.0));
    assert_eq!(reservation["ticket"], id);
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim_reserved(&reservation, "local_b"),
            at(7.0)
        )
        .0,
        200
    );

    // The second fix lands and tip verification passes all seven checks.
    let second_fix = file(&mut conn, &[("fix the board budget", &[], &[])]).remove(0);
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim(second_fix.as_str(), "local_fix"),
            at(8.0)
        )
        .0,
        200
    );
    assert_eq!(
        post(
            &mut conn,
            "/landed",
            &landed(second_fix.as_str(), "local_fix", "fixed"),
            at(9.0)
        )
        .0,
        200
    );
    conn.execute(
        "INSERT INTO records (id, machine, verb, class, queued_s, wall_s, cpu_s, peak_mb,
          commit_sha, ignite, at)
         VALUES (1, 'm-1', 'tip', 'tip', 0, 0, 0, 0, '3a9e4c1', '1111111', ?1)",
        [at(10.0).to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO results (check_name, hash, outcome, machine, record, at, last_lookup)
         VALUES (?1, ?2, 'pass', 'm-1', 1, ?3, ?3)",
        rusqlite::params![boards, key(boards).hash.as_str(), at(10.0).to_string()],
    )
    .unwrap();
    let all: Vec<_> = rust.into_iter().chain([boards]).collect();
    verify(&mut conn, &verified("3333333", &all), 11.0);
    let held = tickets::load(&conn, &id).unwrap().unwrap();
    assert_eq!(held.status, Status::InProgress);
    let comments = tickets::comments(&conn, &id).unwrap();
    let message = comments.last().unwrap();
    assert_eq!(held.metadata["tell"], json!(message.n));
    assert!(message.text.contains("3333333"));
    assert!(message.text.contains("`ignite close`"));
    assert!(message.text.contains(second_fix.as_str()));
    Upkeep::new(None)
        .unwrap()
        .pass(&mut conn, at(12.0))
        .unwrap();
    assert_eq!(tickets::comments(&conn, &id).unwrap().len(), comments.len());

    // The holder releases, which would have crossed the repeated-release threshold.
    assert_eq!(
        post(&mut conn, "/release", &release(&id, "local_b"), at(13.0)).0,
        200
    );
    let closed = tickets::load(&conn, &id).unwrap().unwrap();
    assert_eq!(closed.status, Status::Closed);
    assert!(closed.close_reason.unwrap().contains("3333333"));
    assert_eq!(releases::of(&conn, &id).unwrap().releases.len(), 1);
    assert!(tips::open_regressions(&conn).unwrap().is_empty());
    assert!(offered(&mut conn, "local_c", at(14.0)).is_none());
    assert!(justiciar_tickets(&conn).is_empty());
}

#[test]
fn dispatch_and_repeated_release_filing_skip_a_passing_regression_before_upkeep() {
    let mut conn = open();
    let id = regression(&mut conn, "rust:check", 1.0);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(2.0));
    assert_eq!(reservation["ticket"], id);

    // Store the proof without calling the resolution hook, as when requests interleave.
    tips::verified(&conn, &verified("2222222", &["rust:check"]), at(3.0)).unwrap();
    assert!(
        offered(&mut conn, "local_a", at(4.0)).is_none(),
        "do not repeat the reservation"
    );
    assert!(
        offered(&mut conn, "local_b", at(5.0)).is_none(),
        "do not offer a new reservation"
    );
    for session in ["local_c", "local_d"] {
        releases::record(
            &conn,
            &id,
            session,
            ReleaseCause::GaveUp,
            "blocked",
            at(6.0),
        )
        .unwrap();
    }
    let evidence = releases::of(&conn, &id).unwrap();
    assert!(evidence.held_back());
    assert!(
        justiciar::released_repeatedly(&mut conn, &id, "regression", &evidence, at(7.0))
            .unwrap()
            .is_none()
    );
    assert!(justiciar_tickets(&conn).is_empty());
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(8.0),
    );
    assert_eq!(status, 409);
    assert_eq!(
        tickets::load(&conn, &id).unwrap().unwrap().status,
        Status::Closed
    );
}

#[test]
fn a_newest_tip_missing_an_enabled_check_cannot_use_an_older_tips_pass() {
    let mut conn = open();
    let id = regression(&mut conn, "rust:one", 1.0);
    assert_eq!(regression(&mut conn, "rust:two", 2.0), id);
    tips::verified(
        &conn,
        &verified("2222222", &["rust:one", "rust:two"]),
        at(3.0),
    )
    .unwrap();
    verify(&mut conn, &verified("3333333", &["rust:one"]), 4.0);
    assert_eq!(
        tickets::load(&conn, &id).unwrap().unwrap().status,
        Status::Open
    );

    conn.execute(
        "INSERT INTO checks (name, family, kind, state, check_group)
         VALUES ('rust:two', 'rust', 'regression', 'off', '')",
        [],
    )
    .unwrap();
    assert_eq!(
        Upkeep::new(None)
            .unwrap()
            .pass(&mut conn, at(5.0))
            .unwrap()
            .tickets_closed
            .as_slice(),
        std::slice::from_ref(&id)
    );
    let closed = tickets::load(&conn, &id).unwrap().unwrap();
    let reason = closed.close_reason.unwrap();
    assert!(reason.contains("3333333"));
    assert!(reason.contains("rust:two counts as passing because it is switched off"));
}
