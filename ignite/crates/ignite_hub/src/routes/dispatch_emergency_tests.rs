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

    let pause = |paused: bool| json!({"machine": Value::Null, "paused": paused});
    post(&mut conn, "/pause", &pause(true), at(4.0));
    post(&mut conn, "/pause", &pause(false), at(5.0));
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
