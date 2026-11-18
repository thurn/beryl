//! Drives each route through [`handle`] on a migrated hub database, with time passed in.

use std::sync::{Arc, Barrier};
use std::thread;

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::config::Os;
use ignite_core::ids::{MachineId, TicketId};
use ignite_core::protocol::hub::{IdleWorker, MachineFree};
use ignite_core::protocol::tickets::{BatchRequest, Dependency, Operation, TicketFields};
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, batch, tickets, tokens};

use super::queue::named_files;
use super::{Request, handle};
use crate::auth::Writer;

fn start() -> Timestamp {
    "2026-11-17T14:00:00.000Z".parse().unwrap()
}

fn at(seconds: f64) -> Timestamp {
    start().plus_seconds(seconds)
}

fn post(conn: &mut Connection, path: &str, body: &Value, now: Timestamp) -> (u16, Value) {
    let request = Request {
        method: "POST".into(),
        path: path.into(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: MachineId::new("m-1").unwrap(),
            actor: "test".into(),
        }),
        body: serde_json::to_vec(body).unwrap(),
    };
    let response = handle(conn, &request, now).expect("a dispatch route");
    (
        response.status,
        serde_json::from_slice(&response.body).unwrap(),
    )
}

/// Files one ticket per `(title, labels, deps)`, with deps naming earlier entries by index.
fn file(conn: &mut Connection, specs: &[(&str, &[&str], &[usize])]) -> Vec<TicketId> {
    let ops = specs
        .iter()
        .map(|(title, labels, deps)| Operation::Create {
            fields: TicketFields {
                title: Some((*title).to_owned()),
                labels: Some(labels.iter().map(|l| (*l).to_owned()).collect()),
                ..TicketFields::default()
            },
            parent: None,
            deps: deps
                .iter()
                .map(|i| Dependency {
                    on: format!("${i}"),
                    kind: None,
                })
                .collect(),
            request_key: None,
        })
        .collect();
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply(conn, &request, "planner", "", start()).unwrap() {
        batch::BatchOutcome::Applied(reply) => reply.results.into_iter().map(|r| r.id).collect(),
        batch::BatchOutcome::Conflict(_) => panic!("filing conflicted"),
    }
}

fn free(machine: &str, sessions: &[&str]) -> Value {
    json!(MachineFree {
        machine: machine.parse().unwrap(),
        os: Os::Macos,
        idle_workers: sessions
            .iter()
            .map(|s| IdleWorker {
                session: s.parse().unwrap(),
                app: "claude".into(),
                models: vec!["opus".into(), "sonnet".into()],
            })
            .collect(),
        free_ticket_trees: 4,
        free_cores: 16.0,
        families: Vec::new(),
    })
}

fn claim(ticket: &str, session: &str) -> Value {
    json!({"ticket": ticket, "machine": "m-1", "session": session, "tree": "tree-01"})
}

fn landed(ticket: &str, session: &str, note: &str) -> Value {
    json!({"ticket": ticket, "machine": "m-1", "session": session,
           "work": {"commits": ["3a9e4c1"], "files": ["src/a.rs"], "note": note}})
}

fn release(ticket: &str, session: &str) -> Value {
    json!({"ticket": ticket, "machine": "m-1", "session": session, "reason": "blocked"})
}

#[test]
fn two_machines_asking_at_once_never_get_one_ticket() {
    let dir = std::env::temp_dir().join(format!("ignite-dispatch-race-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hub.sqlite");
    let _ = std::fs::remove_file(&path);
    let mut conn = hub::open(&path).unwrap();
    let specs: Vec<(String, &[&str], &[usize])> = (0..12)
        .map(|i| (format!("ticket {i}"), &[][..], &[][..]))
        .collect();
    let specs: Vec<(&str, &[&str], &[usize])> =
        specs.iter().map(|(t, l, d)| (t.as_str(), *l, *d)).collect();
    file(&mut conn, &specs);
    let barrier = Arc::new(Barrier::new(2));
    let askers: Vec<_> = ["m-a", "m-b"]
        .into_iter()
        .map(|machine| {
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            thread::spawn(move || {
                let mut conn = hub::open(&path).unwrap();
                barrier.wait();
                let mut got = Vec::new();
                for round in 0..10 {
                    let session = format!("local_{machine}_{round}");
                    let (status, reply) =
                        post(&mut conn, "/next", &free(machine, &[&session]), start());
                    assert_eq!(status, 200, "{reply}");
                    if let Some(ticket) = reply["ticket"].as_str() {
                        got.push(ticket.to_owned());
                    }
                }
                got
            })
        })
        .collect();
    let mut all: Vec<String> = askers
        .into_iter()
        .flat_map(|asker| asker.join().unwrap())
        .collect();
    let handed = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), handed, "a ticket was reserved twice");
    assert_eq!(handed, 12);
    drop(conn);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_lapsed_reservation_is_offered_again() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("only", &[], &[])]).remove(0);
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(first["ticket"], id.as_str());
    assert_eq!(first["session"], "local_a");
    let (_, reserved) = post(&mut conn, "/next", &free("m-2", &["local_b"]), at(599.0));
    assert_eq!(reserved, Value::Null);
    let (_, again) = post(&mut conn, "/next", &free("m-2", &["local_b"]), at(600.0));
    assert_eq!(again["ticket"], id.as_str());
    assert_eq!(again["session"], "local_b");
    // The first session's late `ignite start` finds the ticket reserved for another.
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim(id.as_str(), "local_a"),
        at(601.0),
    );
    assert_eq!(status, 409);
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim(id.as_str(), "local_b"),
        at(602.0),
    );
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["ticket"]["session"], "local_b");
    assert_eq!(brief["ticket"]["tree"], "tree-01");
    assert_eq!(brief["ticket"]["status"], "in_progress");
}

#[test]
fn a_worker_with_a_reservation_is_not_offered_another() {
    let mut conn = hub::open_in_memory().unwrap();
    file(&mut conn, &[("one", &[], &[]), ("two", &[], &[])]);
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert!(first["ticket"].is_string());
    let (_, second) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1.0));
    assert_eq!(second, Value::Null);
}

#[test]
fn landing_reserves_the_chain_and_its_brief_carries_the_note() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[("first", &["lane:x"], &[]), ("second", &["lane:x"], &[0])],
    );
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reservation["ticket"], ids[0].as_str());
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim(ids[0].as_str(), "local_a"),
        at(5.0),
    );
    assert_eq!(status, 200);
    let note = "Named counters draw as rows above the plaque.";
    let (status, reply) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", note),
        at(60.0),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["next"]["ticket"], ids[1].as_str());
    assert_eq!(reply["next"]["session"], "local_a");
    let first = tickets::load(&conn, ids[0].as_str()).unwrap().unwrap();
    assert_eq!(first.close_reason.as_deref(), Some(note));
    assert_eq!(first.landed.unwrap().commits[0].as_str(), "3a9e4c1");
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim(ids[1].as_str(), "local_a"),
        at(61.0),
    );
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["needs"][0]["ticket"], ids[0].as_str());
    assert_eq!(brief["needs"][0]["landed"]["note"], note);
    // A resent landing answers as the first did.
    let (status, _) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", note),
        at(62.0),
    );
    assert_eq!(status, 200);
}

#[test]
fn a_session_whose_claim_was_lost_gets_409() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("contested", &[], &[])]).remove(0);
    let id = id.as_str();
    assert_eq!(
        post(&mut conn, "/claims", &claim(id, "local_a"), start()).0,
        200
    );
    assert_eq!(
        post(&mut conn, "/claims", &claim(id, "local_b"), at(1.0)).0,
        409
    );
    assert_eq!(
        post(&mut conn, "/release", &release(id, "local_a"), at(2.0)).0,
        200
    );
    assert_eq!(
        post(&mut conn, "/claims", &claim(id, "local_b"), at(3.0)).0,
        200
    );
    for (path, body) in [
        ("/landed", landed(id, "local_a", "late")),
        ("/release", release(id, "local_a")),
        ("/claims", claim(id, "local_a")),
    ] {
        let (status, reply) = post(&mut conn, path, &body, at(4.0));
        assert_eq!(status, 409, "{path}: {reply}");
        assert_eq!(reply["current"]["assignee"], "local_b");
    }
}

#[test]
fn a_released_ticket_returns_to_the_queue_with_a_comment() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("given back", &[], &[])]).remove(0);
    post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    post(
        &mut conn,
        "/claims",
        &claim(id.as_str(), "local_a"),
        at(1.0),
    );
    let (status, _) = post(
        &mut conn,
        "/release",
        &release(id.as_str(), "local_a"),
        at(2.0),
    );
    assert_eq!(status, 200);
    let ticket = tickets::load(&conn, id.as_str()).unwrap().unwrap();
    assert_eq!(ticket.assignee, None);
    assert_eq!(ticket.session, None);
    assert_eq!(ticket.tree, None);
    let comments = tickets::comments(&conn, id.as_str()).unwrap();
    assert_eq!(comments[0].text, "Released: blocked");
    let (_, again) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(3.0));
    assert_eq!(again["ticket"], id.as_str());
    // An unclaimed reservation can be given back too.
    assert_eq!(
        post(
            &mut conn,
            "/release",
            &release(id.as_str(), "local_b"),
            at(4.0)
        )
        .0,
        200
    );
    let (_, third) = post(&mut conn, "/next", &free("m-1", &["local_c"]), at(5.0));
    assert_eq!(third["ticket"], id.as_str());
}

#[test]
fn a_summary_files_and_claims_an_adhoc_ticket() {
    let mut conn = hub::open_in_memory().unwrap();
    let body = json!({"summary": "Fix the flaky badge test", "machine": "m-1",
                      "session": "local_a", "tree": "tree-02"});
    let (status, brief) = post(&mut conn, "/claims", &body, start());
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["ticket"]["title"], "Fix the flaky badge test");
    assert_eq!(brief["ticket"]["labels"], json!(["adhoc"]));
    assert_eq!(brief["ticket"]["assignee"], "local_a");
    assert_eq!(brief["ticket"]["tree"], "tree-02");
    assert_eq!(brief["needs"], json!([]));
}

#[test]
fn registration_issues_a_token_and_heartbeats_need_a_registered_machine() {
    let mut conn = hub::open_in_memory().unwrap();
    let beat = json!({"machine": "m-7f3a", "version": "d97ed0e", "workers": []});
    assert_eq!(
        post(&mut conn, "/machines/heartbeat", &beat, start()).0,
        404
    );
    let register = json!({"machine": "m-7f3a", "name": "host", "os": "macos", "cores": 18,
                          "memory_mb": 65536, "families": [], "version": "d97ed0e"});
    let (status, reply) = post(&mut conn, "/machines/register", &register, start());
    assert_eq!(status, 200, "{reply}");
    let token = reply["token"].as_str().unwrap();
    assert_eq!(
        tokens::verify(&conn, token).unwrap().unwrap().as_str(),
        "m-7f3a"
    );
    let (status, reply) = post(&mut conn, "/machines/heartbeat", &beat, at(1.0));
    assert_eq!(status, 200);
    assert_eq!(reply["hub_version"], "d97ed0e");
    super::set_hub_version(&conn, "e1f2a3b").unwrap();
    let (_, reply) = post(&mut conn, "/machines/heartbeat", &beat, at(2.0));
    assert_eq!(reply["hub_version"], "e1f2a3b");
}

#[test]
fn writes_need_a_writer_and_other_paths_are_not_ours() {
    let mut conn = hub::open_in_memory().unwrap();
    let request = |path: &str, method: &str| Request {
        method: method.into(),
        path: path.into(),
        query: Vec::new(),
        writer: None,
        body: b"{}".to_vec(),
    };
    let status = |conn: &mut Connection, path: &str, method: &str| {
        handle(conn, &request(path, method), start()).map(|r| r.status)
    };
    assert_eq!(status(&mut conn, "/next", "POST"), Some(401));
    assert_eq!(status(&mut conn, "/claims", "GET"), Some(405));
    assert_eq!(status(&mut conn, "/tickets", "GET"), None);
    // Registration needs no token, so a bad body reaches the parser.
    assert_eq!(status(&mut conn, "/machines/register", "POST"), Some(400));
}

#[test]
fn a_ticket_names_backquoted_paths() {
    let text = "Edit `src/table/badges.rs` and `docs/a.md`, not `cargo test` or \
                `https://x.y/z.html` or `src/` or `src/table/badges.rs` again.";
    assert_eq!(named_files(text), vec!["src/table/badges.rs", "docs/a.md"]);
}

#[test]
fn an_open_regression_goes_before_older_work() {
    let mut conn = hub::open_in_memory().unwrap();
    let ids = file(
        &mut conn,
        &[("old work", &[], &[]), ("fix a check", &[], &[])],
    );
    conn.execute(
        "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
         VALUES ('rust:a::b', ?1, 'b1', 'a1', 'open')",
        [ids[1].as_str()],
    )
    .unwrap();
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reservation["ticket"], ids[1].as_str());
    let kind: String = conn
        .query_row("SELECT kind FROM handoffs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kind, "regression");
}
