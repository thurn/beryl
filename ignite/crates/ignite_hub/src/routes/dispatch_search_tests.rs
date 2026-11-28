//! A culprit search that a slow machine hands off goes to the fastest free machine with the
//! system the check needs, and returns to the pool when its claim expires
//! (`upkeep::regressions::searcher`).

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::ids::MachineId;
use ignite_core::protocol::hub::{FoundBy, LandingRegression, RegressionReport};
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;

use super::Request;
use super::placement_tests::{capacity, record};
use super::speed_tests::fleet;
use super::tests::at;
use crate::auth::Writer;
use crate::upkeep::regressions::searcher::CLAIM_S;

/// Sends a regression request as `machine` (`m-1` jarvis and `m-2` multivac on macOS, `m-3`
/// cygnus on Windows).
fn send(
    conn: &mut Connection,
    method: &str,
    path: &str,
    machine: &str,
    body: &Value,
    now: Timestamp,
) -> Value {
    let (path, query) = match path.split_once('?') {
        Some((path, query)) => {
            let (key, value) = query.split_once('=').unwrap();
            (path, vec![(key.to_owned(), value.to_owned())])
        }
        None => (path, Vec::new()),
    };
    let request = Request {
        method: method.into(),
        path: path.into(),
        query,
        writer: Some(Writer {
            machine: MachineId::new(machine).unwrap(),
            actor: "test".into(),
        }),
        body: serde_json::to_vec(body).unwrap(),
        headers: Vec::new(),
    };
    let response = super::super::checks::handle(conn, &request, now).expect("a regression route");
    let reply = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response.status, 200, "{reply}");
    reply
}

fn searcher(conn: &mut Connection, machine: &str, now: Timestamp) -> Value {
    send(
        conn,
        "GET",
        "/regressions/searcher?check=rust:a",
        machine,
        &Value::Null,
        now,
    )
}

fn next(conn: &mut Connection, machine: &str, now: Timestamp) -> Value {
    send(
        conn,
        "GET",
        "/regressions/landing",
        machine,
        &Value::Null,
        now,
    )
}

/// The failure of `rust:a` that multivac saw at tip `abcdef1`, handed to `searcher`.
fn handed_off(searcher: &Value) -> LandingRegression {
    let tip = "abcdef1".parse().unwrap();
    LandingRegression {
        report: RegressionReport {
            check: "rust:a".parse().unwrap(),
            first_bad: "abcdef1".parse().unwrap(),
            last_good: "abcdef0".parse().unwrap(),
            caused_by: None,
            output: "failed".to_owned(),
            suspects: Vec::new(),
            platform: None,
            search: Some(CulpritSearch::NotRun {
                why: serde_json::from_value::<ignite_core::protocol::hub::CulpritSearcher>(
                    searcher.clone(),
                )
                .unwrap()
                .why(),
            }),
            found_by: FoundBy::TipVerification,
            detected_at: Some(tip),
        },
        inputs: vec!["src/a.rs".to_owned()],
    }
}

fn finished(pending: &Value) -> Value {
    let mut result = pending.clone();
    result["report"] = pending["baseline"]["report"].clone();
    result["report"]["search"] = json!({"kind": "one_candidate"});
    result.as_object_mut().unwrap().remove("baseline");
    result
}

#[test]
fn a_slow_machine_hands_its_search_to_the_fastest_machine_with_its_system() {
    let mut conn = fleet(["1.0", "0.4", "1.0"], at(1.0));
    assert_eq!(searcher(&mut conn, "m-1", at(1.0)), Value::Null);
    let to = searcher(&mut conn, "m-2", at(1.0));
    assert_eq!(to["machine"], "jarvis");
    let failure = handed_off(&to);
    assert!(
        format!("{:?}", failure.report.search).contains("handed to jarvis"),
        "{:?}",
        failure.report.search
    );
    send(
        &mut conn,
        "POST",
        "/regressions/handoff",
        "m-2",
        &json!(failure),
        at(2.0),
    );
    // Neither the slow reporter nor the Windows machine takes it; jarvis does.
    assert_eq!(next(&mut conn, "m-2", at(3.0)), Value::Null);
    assert_eq!(next(&mut conn, "m-3", at(3.0)), Value::Null);
    let pending = next(&mut conn, "m-1", at(3.0));
    assert_eq!(pending["baseline"], json!(failure));
    // Only the claimant's report finishes the search.
    let result = finished(&pending);
    send(
        &mut conn,
        "POST",
        "/regressions/attribution",
        "m-2",
        &result,
        at(4.0),
    );
    assert_eq!(next(&mut conn, "m-1", at(5.0)), pending);
    send(
        &mut conn,
        "POST",
        "/regressions/attribution",
        "m-1",
        &result,
        at(6.0),
    );
    assert_eq!(next(&mut conn, "m-1", at(7.0)), Value::Null);
}

#[test]
fn a_machine_nearly_as_fast_as_the_fastest_searches_itself() {
    let mut conn = fleet(["1.0", "0.8", "1.0"], at(1.0));
    assert_eq!(searcher(&mut conn, "m-2", at(1.0)), Value::Null);
}

#[test]
fn a_search_whose_claim_expired_returns_to_the_pool() {
    let mut conn = fleet(["1.0", "0.4", "1.0"], at(1.0));
    let to = searcher(&mut conn, "m-2", at(1.0));
    send(
        &mut conn,
        "POST",
        "/regressions/handoff",
        "m-2",
        &json!(handed_off(&to)),
        at(2.0),
    );
    let pending = next(&mut conn, "m-1", at(3.0));
    assert_ne!(pending, Value::Null);
    // Jarvis went silent after claiming it. Before the claim expires nobody else takes it.
    let later = at(3.0 + CLAIM_S / 2.0);
    record(&conn, "m-2", &capacity(true), later);
    assert_eq!(next(&mut conn, "m-2", later), Value::Null);
    let expired = at(4.0 + CLAIM_S);
    record(&conn, "m-2", &capacity(true), expired);
    assert_eq!(next(&mut conn, "m-2", expired), pending);
    let result = finished(&pending);
    send(
        &mut conn,
        "POST",
        "/regressions/attribution",
        "m-1",
        &result,
        expired,
    );
    assert_eq!(next(&mut conn, "m-2", expired), pending);
    send(
        &mut conn,
        "POST",
        "/regressions/attribution",
        "m-2",
        &result,
        expired,
    );
    assert_eq!(next(&mut conn, "m-2", expired), Value::Null);
}
