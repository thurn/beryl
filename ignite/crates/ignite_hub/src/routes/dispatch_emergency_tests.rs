//! The emergency pause at the hub: declared and renewed with `POST /emergency`, sent in each
//! heartbeat's reply while it is in force, and ended by resuming every machine.

use serde_json::{Value, json};

use ignite_core::protocol::emergency::EMERGENCY_LASTS_S;
use ignite_store::hub;

use super::tests::{Parent, at, claim, file_under, free, open, post, register};

fn declare(ticket: &str, session: &str) -> Value {
    json!({"change": "declare", "ticket": ticket, "session": session, "machine": "m-1"})
}

fn beat() -> Value {
    json!({"machine": "m-1", "version": "d97ed0e", "workers": []})
}

#[test]
fn a_declared_emergency_rides_every_heartbeat_until_every_machine_resumes() {
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

    let pause =
        |paused: bool| json!({"machine": Value::Null, "paused": paused, "session": "local_i"});
    post(&mut conn, "/pause", &pause(true), at(4.0));
    let (status, resumed) = post(&mut conn, "/pause", &pause(false), at(5.0));
    assert_eq!(status, 200, "{resumed}");
    let (_, after) = post(&mut conn, "/machines/heartbeat", &beat(), at(6.0));
    assert!(after.get("emergency").is_none(), "{after}");
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
fn resuming_every_machine_does_not_end_an_emergency_declared_on_another_ticket() {
    let (mut conn, ids) = two_commanders();
    post(
        &mut conn,
        "/emergency",
        &declare(&ids[1], "local_b"),
        at(3.0),
    );
    let resume = |session: Value, named: Value| json!({"machine": Value::Null, "paused": false, "session": session, "emergency": named});
    let in_force = |conn: &mut rusqlite::Connection, at_s: f64| {
        post(conn, "/machines/heartbeat", &beat(), at(at_s))
            .1
            .get("emergency")
            .is_some()
    };

    // The replaced declarer's resume is refused, with or without a session, and a wrong name
    // does not help.
    for (session, named) in [
        (json!("local_a"), Value::Null),
        (Value::Null, Value::Null),
        (json!("local_a"), json!(ids[0])),
    ] {
        let (status, refused) = post(&mut conn, "/pause", &resume(session, named), at(4.0));
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
        "/pause",
        &resume(json!("local_a"), json!(ids[1])),
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
        "/pause",
        &resume(json!("local_b"), Value::Null),
        at(9.0),
    );
    assert_eq!(status, 200, "{ended}");
    assert!(!in_force(&mut conn, 10.0));
}
