//! The emergency pause at the hub: declared, renewed, adopted and ended with `POST /emergency`,
//! and sent in each heartbeat's reply while it is in force. Resuming every machine's hand-offs
//! leaves it alone.

use serde_json::{Value, json};

use ignite_core::protocol::emergency::EMERGENCY_LASTS_S;
use ignite_core::protocol::tickets::Status;
use ignite_store::hub;

use super::tests::{Parent, at, claim, file_under, free, landed, open, post, register};

fn declare(ticket: &str, session: &str) -> Value {
    json!({"change": "declare", "ticket": ticket, "session": session, "machine": "m-1"})
}

fn end(session: &Value, ticket: &Value) -> Value {
    json!({"change": "end", "session": session, "ticket": ticket})
}

fn beat() -> Value {
    json!({"machine": "m-1", "version": "d97ed0e", "workers": []})
}

#[test]
fn a_declared_emergency_rides_every_heartbeat_until_it_is_ended() {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[("Incident", &["incident"], &[])],
    );
    let ticket = ids[0].as_str();
    assert!(ticket.starts_with('I'));
    let (_, quiet) = post(&mut conn, "/machines/heartbeat", &beat(), at(1.0));
    assert!(quiet.get("emergency").is_none(), "{quiet}");

    let (status, reply) = post(&mut conn, "/claims", &claim(ticket, "local_i"), at(1.0));
    assert_eq!(status, 200, "{reply}");
    let (status, reply) = post(
        &mut conn,
        "/emergency",
        &declare(&ticket[1..], "local_i"),
        at(2.0),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["emergency"]["session"], "local_i");
    let (_, during) = post(&mut conn, "/machines/heartbeat", &beat(), at(3.0));
    assert_eq!(during["emergency"]["ticket"], ticket, "{during}");
    assert_eq!(during["emergency"]["scope"], "all", "{during}");
    assert_eq!(during["paused_everywhere"], true, "{during}");

    let (status, ended) = post(
        &mut conn,
        "/emergency",
        &end(&json!("local_i"), &Value::Null),
        at(5.0),
    );
    assert_eq!(status, 200, "{ended}");
    assert!(ended["emergency"].is_null(), "{ended}");
    let (_, after) = post(&mut conn, "/machines/heartbeat", &beat(), at(6.0));
    assert!(after.get("emergency").is_none(), "{after}");
    assert!(after.get("paused_everywhere").is_none(), "{after}");
}

/// In November 2026 an incident commander resumed hand-offs with `ignite pool resume --all`
/// while upkeep's landings freeze was in force, and that ended the freeze too.
#[test]
fn resuming_every_machine_leaves_a_landings_only_emergency_in_force() {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[("Incident", &["incident"], &[])],
    );
    let incident = ids[0].as_str();
    assert_eq!(
        post(&mut conn, "/claims", &claim(incident, "local_i"), at(1.0)).0,
        200
    );
    let mut declaration = declare(incident, "local_i");
    declaration["scope"] = json!("landings");
    assert_eq!(post(&mut conn, "/emergency", &declaration, at(2.0)).0, 200);

    let pause =
        |paused: bool| json!({"machine": Value::Null, "paused": paused, "session": "local_i"});
    assert_eq!(post(&mut conn, "/pause", &pause(true), at(3.0)).0, 200);
    let (_, paused) = post(&mut conn, "/machines/heartbeat", &beat(), at(4.0));
    assert_eq!(paused["paused_everywhere"], true, "{paused}");
    let (status, resumed) = post(&mut conn, "/pause", &pause(false), at(5.0));
    assert_eq!(status, 200, "{resumed}");
    let (_, after) = post(&mut conn, "/machines/heartbeat", &beat(), at(6.0));
    assert_eq!(after["emergency"]["ticket"], incident, "{after}");
    assert_eq!(after["emergency"]["scope"], "landings", "{after}");
    assert!(after.get("paused_everywhere").is_none(), "{after}");
}

/// A regression filed during a pause of every machine sat unclaimed for 36 minutes in November
/// 2026. The pause that keeps the tickets holding `master` back moving hands it off.
#[test]
fn a_pause_except_holding_master_hands_off_a_regression_and_nothing_else() {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[("Feature", &[], &[]), ("Fix a check", &[], &[])],
    );
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('rust:a::b', ?1, 'b1', 'a1', 'open')",
        [ids[1].as_str()],
    )
    .unwrap();
    let pause = json!({"machine": Value::Null, "paused": true, "except_holding_master": true});
    assert_eq!(post(&mut conn, "/pause", &pause, at(1.0)).0, 200);
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &beat(), at(2.0));
    assert_eq!(reply["paused_except_master"], true, "{reply}");
    assert!(reply.get("paused_everywhere").is_none(), "{reply}");

    let (status, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(3.0));
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["ticket"], ids[1].as_str(), "{first}");
    let (_, nothing) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(4.0));
    assert_eq!(nothing, Value::Null);

    let resume = json!({"machine": Value::Null, "paused": false});
    assert_eq!(post(&mut conn, "/pause", &resume, at(5.0)).0, 200);
    let (_, feature) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(6.0));
    assert_eq!(feature["ticket"], ids[0].as_str(), "{feature}");
}

/// An incident commander adopts upkeep's landings freeze: it keeps its allowances, upkeep goes
/// on renewing it, and it still ends once `master` catches up.
#[test]
fn an_adopted_landings_freeze_keeps_its_allowances_and_ends_when_master_catches_up() {
    use crate::upkeep::freeze::{self, FREEZE_SECONDS};
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[
            ("Work", &[], &[]),
            ("Fix the tip", &["regression"], &[]),
            ("Incident", &["incident"], &[]),
        ],
    );
    let (work, fix, incident) = (ids[0].as_str(), ids[1].as_str(), ids[2].as_str());
    conn.execute(
        "UPDATE tickets SET landed_at = ?1 WHERE id = ?2",
        rusqlite::params![at(0.0).to_string(), work],
    )
    .unwrap();
    let report = freeze::pass(&mut conn, at(FREEZE_SECONDS)).unwrap();
    assert!(report.frozen);
    let frozen = report.filed[0].ticket.clone();
    let declared = hub::emergency::in_force(&conn, at(FREEZE_SECONDS))
        .unwrap()
        .unwrap();
    assert_eq!(declared.ticket.as_str(), frozen);

    let later = |s: f64| at(FREEZE_SECONDS + s);
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim(incident, "local_i"),
            later(1.0)
        )
        .0,
        200
    );
    let adopt = json!({"change": "adopt", "ticket": incident, "session": "local_i",
                       "machine": "m-1"});
    let (status, adopted) = post(&mut conn, "/emergency", &adopt, later(2.0));
    assert_eq!(status, 200, "{adopted}");
    let adopted = &adopted["emergency"];
    assert_eq!(adopted["ticket"], incident, "{adopted}");
    assert_eq!(adopted["session"], "local_i", "{adopted}");
    assert_eq!(adopted["scope"], "landings", "{adopted}");
    let allowed: Vec<&str> = adopted["allowed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["ticket"].as_str().unwrap())
        .collect();
    assert!(allowed.contains(&fix), "{adopted}");
    assert!(allowed.contains(&frozen.as_str()), "{adopted}");
    for ticket in [frozen.as_str(), incident] {
        let comments = hub::tickets::comments(&conn, ticket).unwrap();
        let recorded = comments.last().expect("the hand-over is recorded");
        assert!(recorded.text.contains("adopted"), "{}", recorded.text);
    }
    // Only a holder of an incident or justiciar ticket may adopt.
    let stranger = json!({"change": "adopt", "ticket": work, "session": "local_w",
                          "machine": "m-1"});
    assert_eq!(post(&mut conn, "/emergency", &stranger, later(3.0)).0, 409);

    // Upkeep renews the adopted freeze past the two hours a declaration lasts.
    freeze::pass(&mut conn, later(3600.0)).unwrap();
    let renewed = hub::emergency::in_force(&conn, later(EMERGENCY_LASTS_S + 60.0))
        .unwrap()
        .expect("upkeep renews the adopted freeze");
    assert_eq!(renewed.ticket.as_str(), incident);

    conn.execute(
        "INSERT INTO tips (commit_sha, seen, verified, state) VALUES ('aaaaaaa', ?1, ?1, 'verified')",
        [later(3700.0).to_string()],
    )
    .unwrap();
    assert!(freeze::pass(&mut conn, later(3700.0)).unwrap().lifted);
    assert!(
        hub::emergency::in_force(&conn, later(3701.0))
            .unwrap()
            .is_none()
    );
}

#[test]
fn worker_hand_offs_resume_when_a_full_emergency_becomes_landings_only() {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[("Incident", &["incident"], &[]), ("Work", &[], &[])],
    );
    let incident = ids[0].as_str();
    assert_eq!(
        post(&mut conn, "/claims", &claim(incident, "local_i"), at(1.0)).0,
        200
    );
    let mut declaration = declare(incident, "local_i");
    assert_eq!(post(&mut conn, "/emergency", &declaration, at(2.0)).0, 200);
    let idle = free("m-1", &["local_w"]);
    assert_eq!(post(&mut conn, "/next", &idle, at(3.0)).1, Value::Null);

    declaration["scope"] = json!("landings");
    let (status, renewed) = post(&mut conn, "/emergency", &declaration, at(4.0));
    assert_eq!(status, 200, "{renewed}");
    assert_eq!(renewed["emergency"]["scope"], "landings", "{renewed}");
    let (_, heartbeat) = post(&mut conn, "/machines/heartbeat", &beat(), at(5.0));
    assert_eq!(heartbeat["emergency"]["scope"], "landings", "{heartbeat}");
    assert!(heartbeat.get("paused_everywhere").is_none(), "{heartbeat}");
    let (status, handoff) = post(&mut conn, "/next", &idle, at(6.0));
    assert_eq!(status, 200, "{handoff}");
    assert_eq!(handoff["ticket"], ids[1].as_str(), "{handoff}");

    let manual_pause = json!({"machine": null, "paused": true});
    assert_eq!(post(&mut conn, "/pause", &manual_pause, at(7.0)).0, 200);
    assert_eq!(post(&mut conn, "/emergency", &declaration, at(8.0)).0, 200);
    let (_, heartbeat) = post(&mut conn, "/machines/heartbeat", &beat(), at(9.0));
    assert_eq!(heartbeat["emergency"]["scope"], "landings", "{heartbeat}");
    assert_eq!(heartbeat["paused_everywhere"], true, "{heartbeat}");
}

#[test]
fn an_emergency_nobody_renews_lapses_and_only_its_justiciar_allows_tickets() {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[("Justiciar", &["justiciar"], &[]), ("Work", &[], &[])],
    );
    let (status, reply) = post(
        &mut conn,
        "/claims",
        &claim(ids[0].as_str(), "local_j"),
        at(0.0),
    );
    assert_eq!(status, 200, "{reply}");
    let (status, reply) = post(
        &mut conn,
        "/emergency",
        &declare(ids[0].as_str(), "local_j"),
        at(0.0),
    );
    assert_eq!(status, 200, "{reply}");
    let allow = |session: &str| {
        json!({"change": "allow", "session": session,
               "allowance": {"ticket": ids[1].as_str(), "verb": "land"}})
    };
    assert_eq!(
        post(&mut conn, "/emergency", &allow("local_w"), at(1.0)).0,
        409
    );
    let (status, reply) = post(&mut conn, "/emergency", &allow("local_j"), at(2.0));
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["emergency"]["allowed"][0]["verb"], "land");

    let lapsed = at(EMERGENCY_LASTS_S + 1.0);
    let (_, after) = post(&mut conn, "/machines/heartbeat", &beat(), lapsed);
    assert!(after.get("emergency").is_none(), "{after}");
    assert!(after.get("paused_everywhere").is_none(), "{after}");
}

#[test]
fn only_the_holder_of_a_labelled_ticket_can_declare_an_emergency() {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[("Incident", &["incident"], &[]), ("Work", &[], &[])],
    );
    let incident = ids[0].as_str();
    let work = ids[1].as_str();
    assert_eq!(
        post(
            &mut conn,
            "/emergency",
            &declare(incident, "local_i"),
            at(0.0)
        )
        .0,
        409
    );
    assert_eq!(
        post(&mut conn, "/claims", &claim(incident, "local_i"), at(1.0)).0,
        200
    );
    assert_eq!(
        post(&mut conn, "/claims", &claim(work, "local_w"), at(2.0)).0,
        200
    );
    for (ticket, session) in [(incident, "local_w"), (work, "local_w")] {
        let (status, reply) = post(&mut conn, "/emergency", &declare(ticket, session), at(3.0));
        assert_eq!(status, 409, "{reply}");
    }
    assert!(hub::emergency::in_force(&conn, at(3.0)).unwrap().is_none());
}

/// Two incident commanders: the second one's declaration replaces the first's.
fn two_commanders() -> (rusqlite::Connection, [String; 2]) {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[
            ("First", &["incident"], &[]),
            ("Second", &["incident"], &[]),
        ],
    );
    let ids = [ids[0].as_str().to_owned(), ids[1].as_str().to_owned()];
    for (id, session) in ids.iter().zip(["local_a", "local_b"]) {
        assert_eq!(
            post(&mut conn, "/claims", &claim(id, session), at(1.0)).0,
            200
        );
    }
    assert_eq!(
        post(
            &mut conn,
            "/emergency",
            &declare(&ids[0], "local_a"),
            at(2.0)
        )
        .0,
        200
    );
    (conn, ids)
}

#[test]
fn a_replaced_declarer_is_told_and_the_allow_refusal_names_the_current_emergency() {
    let (mut conn, ids) = two_commanders();
    let (status, replaced) = post(
        &mut conn,
        "/emergency",
        &declare(&ids[1], "local_b"),
        at(3.0),
    );
    assert_eq!(status, 200, "{replaced}");

    let first = hub::tickets::load(&conn, &ids[0]).unwrap().unwrap();
    let comments = hub::tickets::comments(&conn, &ids[0]).unwrap();
    let told = comments.last().expect("the replaced declarer is commented");
    assert!(
        told.text.contains(&ids[1]) && told.text.contains("local_b"),
        "{}",
        told.text
    );
    assert_eq!(first.metadata["tell"], json!(told.n));

    let allow = json!({"change": "allow", "session": "local_a",
                       "allowance": {"ticket": ids[1], "verb": null}});
    let (status, refused) = post(&mut conn, "/emergency", &allow, at(4.0));
    assert_eq!(status, 409, "{refused}");
    let message = refused["error"].as_str().unwrap();
    assert!(
        message.contains(&ids[1]) && message.contains("local_b"),
        "{message}"
    );

    // Renewing one's own emergency tells nobody.
    let before = hub::tickets::comments(&conn, &ids[1]).unwrap().len();
    post(
        &mut conn,
        "/emergency",
        &declare(&ids[1], "local_b"),
        at(5.0),
    );
    assert_eq!(
        hub::tickets::comments(&conn, &ids[1]).unwrap().len(),
        before
    );
}

#[test]
fn ending_an_emergency_declared_on_another_ticket_needs_its_name() {
    let (mut conn, ids) = two_commanders();
    post(
        &mut conn,
        "/emergency",
        &declare(&ids[1], "local_b"),
        at(3.0),
    );
    let in_force = |conn: &mut rusqlite::Connection, at_s: f64| {
        post(conn, "/machines/heartbeat", &beat(), at(at_s))
            .1
            .get("emergency")
            .is_some()
    };

    // The replaced declarer's end is refused, with or without a session, and a wrong name
    // does not help.
    for (session, named) in [
        (json!("local_a"), Value::Null),
        (Value::Null, Value::Null),
        (json!("local_a"), json!(ids[0])),
    ] {
        let (status, refused) = post(&mut conn, "/emergency", &end(&session, &named), at(4.0));
        assert_eq!(status, 409, "{refused}");
        assert!(
            refused["error"].as_str().unwrap().contains(&ids[1]),
            "{refused}"
        );
    }
    assert!(in_force(&mut conn, 5.0));

    // Naming the emergency's ticket ends it, as does holding that ticket.
    let (status, ended) = post(
        &mut conn,
        "/emergency",
        &end(&json!("local_a"), &json!(ids[1])),
        at(6.0),
    );
    assert_eq!(status, 200, "{ended}");
    assert!(!in_force(&mut conn, 7.0));
    post(
        &mut conn,
        "/emergency",
        &declare(&ids[1], "local_b"),
        at(8.0),
    );
    let (status, ended) = post(
        &mut conn,
        "/emergency",
        &end(&json!("local_b"), &Value::Null),
        at(9.0),
    );
    assert_eq!(status, 200, "{ended}");
    assert!(!in_force(&mut conn, 10.0));
}

/// In November 2026 a justiciar landed its last fix under the landings freeze's ticket, and
/// the landing closed it while `master` was still 71 landings behind. A landing under that
/// ticket now records the work and releases the ticket, so the pool offers it again.
#[test]
fn landing_under_an_unended_freeze_releases_its_ticket_instead_of_closing_it() {
    let mut conn = open();
    register(&mut conn);
    let (_, ids) = file_under(
        &mut conn,
        Parent::Loose,
        &[("Landings frozen", &["justiciar"], &[])],
    );
    let ticket = ids[0].as_str();
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('master_freeze', ?1)",
        [json!({"ticket": ticket, "since": at(0.0)}).to_string()],
    )
    .unwrap();
    let (status, reply) = post(&mut conn, "/claims", &claim(ticket, "local_j"), at(1.0));
    assert_eq!(status, 200, "{reply}");
    let (status, reply) = post(
        &mut conn,
        "/landed",
        &landed(ticket, "local_j", "fix: the tip"),
        at(2.0),
    );
    assert_eq!(status, 200, "{reply}");
    let after = ignite_store::hub::tickets::load(&conn, ticket)
        .unwrap()
        .unwrap();
    assert_eq!(after.status, Status::Open);
    assert_eq!(after.assignee, None);
    assert!(after.landed.is_some());
    let comments: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM comments WHERE ticket = ?1 AND text LIKE 'Landed 3a9e4c1%'",
            [ticket],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(comments, 1);
    // The daemon sends a landing again when it did not read the reply.
    let (status, reply) = post(
        &mut conn,
        "/landed",
        &landed(ticket, "local_j", "fix: the tip"),
        at(2.5),
    );
    assert_eq!(status, 200, "{reply}");

    // Once `master` catches up, upkeep deletes the stored freeze and a landing closes again.
    conn.execute("DELETE FROM meta WHERE key = 'master_freeze'", [])
        .unwrap();
    post(&mut conn, "/claims", &claim(ticket, "local_k"), at(3.0));
    let (status, reply) = post(
        &mut conn,
        "/landed",
        &landed(ticket, "local_k", "fix: more"),
        at(4.0),
    );
    assert_eq!(status, 200, "{reply}");
    let closed = ignite_store::hub::tickets::load(&conn, ticket)
        .unwrap()
        .unwrap();
    assert_eq!(closed.status, Status::Closed);
}
