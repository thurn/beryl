use rusqlite::{Connection, params};

use ignite_core::inputs::{CrateGraph, Package};
use ignite_core::time::Timestamp;

use super::since;
use crate::hub;

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn graph() -> CrateGraph {
    CrateGraph {
        packages: vec![Package {
            name: "core".to_owned(),
            dir: "rust/core".to_owned(),
            deps: Vec::new(),
            workspace: "rust".to_owned(),
        }],
    }
}

fn check(conn: &Connection, name: &str, kind: &str) {
    let family = name.split(':').next().unwrap();
    conn.execute(
        "INSERT INTO checks (name, family, kind, check_group) VALUES (?1, ?2, ?3, ?2)",
        params![name, family, kind],
    )
    .unwrap();
}

fn ticket(conn: &Connection, id: &str, created: &str, landed: Option<(&str, &[&str], &[&str])>) {
    let (landed_at, landed) = match landed {
        Some((when, commits, files)) => (
            Some(when),
            Some(serde_json::json!({"commits": commits, "files": files, "note": ""}).to_string()),
        ),
        None => (None, None),
    };
    conn.execute(
        "INSERT INTO tickets (id, title, status, created_at, created_by, updated_at, version,
                              landed_at, landed)
         VALUES (?1, ?1, 'closed', ?2, 't', ?2, 0, ?3, ?4)",
        params![id, created, landed_at, landed],
    )
    .unwrap();
}

fn record(conn: &Connection, machine: &str, family: &str, ticket: &str, outcome: &str, when: &str) {
    conn.execute(
        "INSERT INTO records (machine, ticket, verb, class, family, queued_s, wall_s, cpu_s,
                              peak_mb, outcome, ignite, at)
         VALUES (?1, ?2, 'land', 'land', ?3, 0, 1, 1, 1, ?4, 'x', ?5)",
        params![machine, ticket, family, outcome, when],
    )
    .unwrap();
}

fn result(conn: &Connection, check: &str, hash: &str, outcome: &str, machine: &str, when: &str) {
    conn.execute(
        "INSERT INTO results (check_name, hash, platform, outcome, machine, record, at,
                              last_lookup)
         VALUES (?1, ?2, '', ?3, ?4, 0, ?5, ?5)",
        params![check, hash, outcome, machine, when],
    )
    .unwrap();
}

#[test]
fn a_failure_fixed_before_landing_is_a_catch_in_the_landed_areas() {
    let conn = hub::open_in_memory().unwrap();
    check(&conn, "rust:a", "regression");
    ticket(
        &conn,
        "t.1",
        "2026-11-10T00:00:00.000Z",
        Some((
            "2026-11-10T05:00:00.000Z",
            &["c1"],
            &["rust/core/src/lib.rs", "docs/x/y.md"],
        )),
    );
    // Another ticket on another machine at the same time, which must not be chosen.
    ticket(&conn, "t.2", "2026-11-10T00:00:00.000Z", None);
    record(
        &conn,
        "m2",
        "rust",
        "t.2",
        "fail",
        "2026-11-10T01:00:00.000Z",
    );
    record(
        &conn,
        "m1",
        "rust",
        "t.1",
        "fail",
        "2026-11-10T01:00:02.000Z",
    );
    result(
        &conn,
        "rust:a",
        "h1",
        "fail",
        "m1",
        "2026-11-10T01:00:01.000Z",
    );
    // A second failure of the same check for the same ticket counts once.
    record(
        &conn,
        "m1",
        "rust",
        "t.1",
        "fail",
        "2026-11-10T02:00:02.000Z",
    );
    result(
        &conn,
        "rust:a",
        "h2",
        "fail",
        "m1",
        "2026-11-10T02:00:01.000Z",
    );
    let found = since(&conn, at("2026-11-01T00:00:00.000Z"), &graph()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].check, "rust:a");
    assert_eq!(found[0].at, at("2026-11-10T01:00:01.000Z"));
    assert_eq!(
        found[0].areas,
        vec!["core".to_owned(), "docs/x/".to_owned()]
    );
}

#[test]
fn flaky_unlanded_exploration_and_old_failures_are_not_catches() {
    let conn = hub::open_in_memory().unwrap();
    check(&conn, "rust:flaky", "regression");
    check(&conn, "rust:open", "regression");
    check(&conn, "rust:old", "regression");
    check(&conn, "scenarios:find", "exploration");
    ticket(
        &conn,
        "t.1",
        "2026-11-10T00:00:00.000Z",
        Some(("2026-11-10T05:00:00.000Z", &["c1"], &["rust/core/a.rs"])),
    );
    ticket(&conn, "t.2", "2026-11-10T00:00:00.000Z", None);
    record(
        &conn,
        "m1",
        "rust",
        "t.1",
        "fail",
        "2026-11-10T01:00:00.000Z",
    );
    record(
        &conn,
        "m1",
        "scenarios",
        "t.1",
        "fail",
        "2026-11-10T01:00:00.000Z",
    );
    record(
        &conn,
        "m2",
        "rust",
        "t.2",
        "fail",
        "2026-11-10T01:00:00.000Z",
    );
    result(
        &conn,
        "rust:flaky",
        "h1",
        "fail",
        "m1",
        "2026-11-10T01:00:00.000Z",
    );
    conn.execute(
        "INSERT INTO flaky (check_name, hash, commit_sha, machine, at)
         VALUES ('rust:flaky', 'h1', 'c0', 'm1', '2026-11-10T01:00:00.000Z')",
        [],
    )
    .unwrap();
    result(
        &conn,
        "rust:open",
        "h1",
        "fail",
        "m2",
        "2026-11-10T01:00:00.000Z",
    );
    result(
        &conn,
        "scenarios:find",
        "h1",
        "fail",
        "m1",
        "2026-11-10T01:00:00.000Z",
    );
    result(
        &conn,
        "rust:old",
        "h1",
        "fail",
        "m1",
        "2026-10-01T01:00:00.000Z",
    );
    // A failure after its ticket landed was not fixed by that landing.
    result(
        &conn,
        "rust:old",
        "h2",
        "fail",
        "m1",
        "2026-11-10T06:00:00.000Z",
    );
    let found = since(&conn, at("2026-11-01T00:00:00.000Z"), &graph()).unwrap();
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_fixed_regression_is_a_catch_in_the_culprits_areas() {
    let conn = hub::open_in_memory().unwrap();
    check(&conn, "rust:a", "regression");
    check(&conn, "rust:b", "regression");
    ticket(
        &conn,
        "t.1",
        "2026-11-09T00:00:00.000Z",
        Some(("2026-11-09T01:00:00.000Z", &["bad"], &["rust/core/a.rs"])),
    );
    ticket(&conn, "r.1", "2026-11-09T02:00:00.000Z", None);
    ticket(&conn, "r.2", "2026-11-09T02:00:00.000Z", None);
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('rust:a', 'r.1', 'bad', 'good', 'fixed'),
                ('rust:b', 'r.2', 'unknown', 'good', 'fixed')",
        [],
    )
    .unwrap();
    let found = since(&conn, at("2026-11-01T00:00:00.000Z"), &graph()).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].check, "rust:a");
    assert_eq!(found[0].areas, vec!["core".to_owned()]);
    assert_eq!(found[1].areas, vec![String::new()]);
    // An open regression has not been fixed yet.
    conn.execute("UPDATE regressions SET state = 'open'", [])
        .unwrap();
    assert!(
        since(&conn, at("2026-11-01T00:00:00.000Z"), &graph())
            .unwrap()
            .is_empty()
    );
}
