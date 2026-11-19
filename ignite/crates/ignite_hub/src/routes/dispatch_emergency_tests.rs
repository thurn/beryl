//! The emergency pause at the hub: declared and renewed with `POST /emergency`, sent in each
//! heartbeat's reply while it is in force, and ended by resuming every machine.

use serde_json::{Value, json};

use ignite_core::protocol::emergency::EMERGENCY_LASTS_S;
use ignite_store::hub;

use super::tests::{at, file, post, register};

fn declare(ticket: &str, session: &str) -> Value {
    json!({"change": "declare", "ticket": ticket, "session": session, "machine": "m-1"})
}

fn beat() -> Value {
    json!({"machine": "m-1", "version": "d97ed0e", "workers": []})
}

#[test]
fn a_declared_emergency_rides_every_heartbeat_until_every_machine_resumes() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ticket = file(&mut conn, &[("justiciar", &[], &[])]).remove(0);
    let (_, quiet) = post(&mut conn, "/machines/heartbeat", &beat(), at(1.0));
    assert!(quiet.get("emergency").is_none(), "{quiet}");

    let (status, reply) = post(
        &mut conn,
        "/emergency",
        &declare(ticket.as_str(), "local_j"),
        at(2.0),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["emergency"]["session"], "local_j");
    let (_, during) = post(&mut conn, "/machines/heartbeat", &beat(), at(3.0));
    assert_eq!(during["emergency"]["ticket"], ticket.as_str(), "{during}");

    let pause = |paused: bool| json!({"machine": Value::Null, "paused": paused});
    post(&mut conn, "/pause", &pause(true), at(4.0));
    post(&mut conn, "/pause", &pause(false), at(5.0));
    let (_, after) = post(&mut conn, "/machines/heartbeat", &beat(), at(6.0));
    assert!(after.get("emergency").is_none(), "{after}");
}

#[test]
fn an_emergency_nobody_renews_lapses_and_only_its_justiciar_allows_tickets() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(&mut conn, &[("justiciar", &[], &[]), ("work", &[], &[])]);
    post(
        &mut conn,
        "/emergency",
        &declare(ids[0].as_str(), "local_j"),
        at(0.0),
    );
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
}
