//! Drives each route through [`handle`] on a migrated hub database, with time passed in.

use std::sync::{Arc, Barrier};
use std::thread;

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::config::Os;
use ignite_core::ids::{MachineId, TicketId};
use ignite_core::protocol::hub::{IdleWorker, MachineFree};
use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, Operation, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, batch, tickets, tokens};

use super::queue::named_files;
use super::{Request, handle};
use crate::auth::Writer;

pub(super) fn start() -> Timestamp {
    "2026-11-17T14:00:00.000Z".parse().unwrap()
}

pub(super) fn at(seconds: f64) -> Timestamp {
    start().plus_seconds(seconds)
}

/// Posts as the machine the body names (the hub refuses a body that names another machine than
/// the token's), or as `m-1` when it names none.
pub(super) fn post(
    conn: &mut Connection,
    path: &str,
    body: &Value,
    now: Timestamp,
) -> (u16, Value) {
    let machine = body["machine"].as_str().unwrap_or("m-1");
    let request = Request {
        method: "POST".into(),
        path: path.into(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: MachineId::new(machine).unwrap(),
            actor: "test".into(),
        }),
        body: serde_json::to_vec(body).unwrap(),
        headers: Vec::new(),
    };
    let response = handle(conn, &request, now).expect("a dispatch route");
    (
        response.status,
        serde_json::from_slice(&response.body).unwrap(),
    )
}

/// Files one ticket per `(title, labels, deps)` under an active epic, with deps naming earlier
/// entries by index.
pub(super) fn file(conn: &mut Connection, specs: &[(&str, &[&str], &[usize])]) -> Vec<TicketId> {
    file_under(conn, Parent::Epic(Some(2)), specs).1
}

/// Where [`file_under`] files its tickets.
#[derive(Clone, Copy)]
pub(super) enum Parent {
    /// Under a new epic with this `workers` number, or with none.
    Epic(Option<u32>),
    /// With no parent.
    Loose,
}

/// Files the tickets under `parent`. Returns the epic, if one was filed, and the tickets.
pub(super) fn file_under(
    conn: &mut Connection,
    parent: Parent,
    specs: &[(&str, &[&str], &[usize])],
) -> (Option<TicketId>, Vec<TicketId>) {
    let workers = match parent {
        Parent::Epic(workers) => Some(workers),
        Parent::Loose => None,
    };
    let offset = usize::from(workers.is_some());
    let mut ops: Vec<Operation> = workers
        .map(|workers| Operation::Create {
            fields: TicketFields {
                title: Some("epic".to_owned()),
                ticket_type: Some(TicketType::Epic),
                workers: Some(workers),
                ..TicketFields::default()
            },
            parent: None,
            deps: Vec::new(),
            request_key: None,
        })
        .into_iter()
        .collect();
    ops.extend(specs.iter().map(|(title, labels, deps)| {
        Operation::Create {
            fields: TicketFields {
                title: Some((*title).to_owned()),
                labels: Some(labels.iter().map(|l| (*l).to_owned()).collect()),
                ..TicketFields::default()
            },
            parent: workers.map(|_| "$0".to_owned()),
            deps: deps
                .iter()
                .map(|i| Dependency {
                    on: format!("${}", i + offset),
                    kind: None,
                })
                .collect(),
            request_key: None,
        }
    }));
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    let mut ids: Vec<TicketId> = match batch::apply(conn, &request, "planner", "", start()).unwrap()
    {
        batch::BatchOutcome::Applied(reply) => reply.results.into_iter().map(|r| r.id).collect(),
        batch::BatchOutcome::Conflict(_) => panic!("filing conflicted"),
    };
    let epic = (offset == 1).then(|| ids.remove(0));
    (epic, ids)
}

/// Registers the machine `m-1`, which every request in these tests is written as.
pub(super) fn register(conn: &mut Connection) {
    let register = json!({"machine": "m-1", "name": "jarvis", "os": "macos", "cores": 18,
                          "memory_mb": 65536, "families": [], "version": "d97ed0e"});
    assert_eq!(post(conn, "/machines/register", &register, start()).0, 200);
}

pub(super) fn free(machine: &str, sessions: &[&str]) -> Value {
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
        admit_cores: None,
        pressure: None,
        families: Vec::new(),
        name: None,
        justiciar_only: false,
        master_only: false,
        explain: false,
    })
}

pub(super) fn claim(ticket: &str, session: &str) -> Value {
    json!({"ticket": ticket, "machine": "m-1", "session": session, "tree": "tree-01"})
}

/// A claim of a reservation with its code, as the hand-off's `ignite start` sends it.
pub(super) fn claim_reserved(reservation: &Value, session: &str) -> Value {
    let mut body = claim(reservation["ticket"].as_str().unwrap(), session);
    body["code"] = reservation["code"].clone();
    body
}

pub(super) fn landed(ticket: &str, session: &str, note: &str) -> Value {
    json!({"ticket": ticket, "machine": "m-1", "session": session,
           "work": {"commits": ["3a9e4c1"], "files": ["src/a.rs"], "note": note}})
}

pub(super) fn release(ticket: &str, session: &str) -> Value {
    json!({"ticket": ticket, "machine": "m-1", "session": session, "reason": "blocked"})
}

#[test]
fn two_machines_asking_at_once_never_get_one_ticket() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-dispatch-race-{}", std::process::id()));
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
    // So does a claim with the lapsed reservation's code.
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&first, "local_a"),
        at(601.0),
    );
    assert_eq!(status, 409);
    let mut on_m2 = claim_reserved(&again, "local_b");
    on_m2["machine"] = json!("m-2");
    let (status, brief) = post(&mut conn, "/claims", &on_m2, at(602.0));
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["ticket"]["session"], "local_b");
    assert_eq!(brief["ticket"]["tree"], "tree-01");
    assert_eq!(brief["ticket"]["status"], "in_progress");
}

#[test]
fn a_worker_asking_again_gets_its_reservation_back_not_another() {
    let mut conn = hub::open_in_memory().unwrap();
    file(&mut conn, &[("one", &[], &[]), ("two", &[], &[])]);
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert!(first["ticket"].is_string());
    // The reply to the first request may have been lost, so the same worker asking again gets
    // the same reservation, and no second ticket.
    let (_, second) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1.0));
    assert_eq!(second, first);
    let (_, other) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(2.0));
    assert_ne!(other["ticket"], first["ticket"]);
}

#[test]
fn landing_reserves_the_chain_and_its_brief_carries_the_note() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(
        &mut conn,
        &[("first", &["lane:x"], &[]), ("second", &["lane:x"], &[0])],
    );
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reservation["ticket"], ids[0].as_str());
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
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
    let (status, retry) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", note),
        at(60.5),
    );
    assert_eq!(status, 200);
    assert_eq!(
        retry, reply,
        "a lost reply must keep the same code and expiry"
    );
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reply["next"], "local_a"),
        at(61.0),
    );
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["needs"][0]["ticket"], ids[0].as_str());
    assert_eq!(brief["needs"][0]["landed"]["note"], note);
    // The claim consumed the reservation, so a replay cannot hand it off again.
    let (status, retry) = post(
        &mut conn,
        "/landed",
        &landed(ids[0].as_str(), "local_a", note),
        at(62.0),
    );
    assert_eq!(status, 200);
    assert!(retry["next"].is_null());
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

/// Under Enact, a metadata write by the dispatcher between the reservation and the worker's start
/// changed the ticket's version, and the start, which checked that version, failed as if another
/// owner held the ticket. Here a claim checks only the reservation and the assignee, so a write
/// that changes neither (a comment and a metadata key, as `ignite tell` writes) cannot fail it.
#[test]
fn a_write_between_reservation_and_claim_does_not_fail_the_holders_claim() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("handed off", &[], &[])]).remove(0);
    let (_, reserved) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(reserved["ticket"], id.as_str());
    let before = tickets::load(&conn, id.as_str()).unwrap().unwrap().version;
    let write = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![
            Operation::Comment {
                id: id.to_string(),
                text: "a message for the worker".into(),
            },
            Operation::Update {
                id: id.to_string(),
                set: None,
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: std::collections::BTreeMap::from([("tell".to_owned(), json!(1))]),
                unset_meta: Vec::new(),
            },
        ],
    };
    assert!(matches!(
        batch::apply(&mut conn, &write, "dispatcher", "m-1", at(1.0)).unwrap(),
        batch::BatchOutcome::Applied(_)
    ));
    assert_ne!(
        tickets::load(&conn, id.as_str()).unwrap().unwrap().version,
        before
    );
    // A session without the code is refused, and the reply says the ticket is reserved.
    let (status, reply) = post(
        &mut conn,
        "/claims",
        &claim(id.as_str(), "local_b"),
        at(2.0),
    );
    assert_eq!(status, 409);
    assert_eq!(
        reply["error"],
        format!(
            "{id} is reserved for a worker the dispatcher is starting; nobody else can claim it, so take another ticket"
        )
    );
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reserved, "local_a"),
        at(3.0),
    );
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["ticket"]["assignee"], "local_a");
    // A start retried after a lost reply is answered with the brief again.
    let (status, again) = post(
        &mut conn,
        "/claims",
        &claim(id.as_str(), "local_a"),
        at(4.0),
    );
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["ticket"]["session"], "local_a");
}

#[test]
fn a_released_ticket_returns_to_the_queue_with_a_comment() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("given back", &[], &[])]).remove(0);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(1.0),
    );
    assert_eq!(status, 200);
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
        headers: Vec::new(),
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

/// Sets the epic's `workers` number.
pub(super) fn set_workers(conn: &mut Connection, epic: &TicketId, workers: u32) {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Update {
            id: epic.as_str().to_owned(),
            set: Some(TicketFields {
                workers: Some(Some(workers)),
                ..TicketFields::default()
            }),
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: std::collections::BTreeMap::new(),
            unset_meta: Vec::new(),
        }],
    };
    assert!(matches!(
        batch::apply(conn, &request, "person", "", start()).unwrap(),
        batch::BatchOutcome::Applied(_)
    ));
}

#[test]
fn only_an_active_epic_loose_or_the_hub_s_own_tickets_are_offered() {
    let mut conn = hub::open_in_memory().unwrap();
    let (epic, waiting) = file_under(&mut conn, Parent::Epic(None), &[("backlog", &[], &[])]);
    let (_, stopped) = file_under(&mut conn, Parent::Epic(Some(0)), &[("stopped", &[], &[])]);
    // A new idle worker each time, since a worker holding a reservation is offered nothing.
    let ask = |conn: &mut Connection, seconds: f64| {
        let session = format!("local_{seconds}");
        post(conn, "/next", &free("m-1", &[&session]), at(seconds)).1
    };
    assert_eq!(ask(&mut conn, 1.0), Value::Null);
    // Tickets the hub files itself are offered with no epic at all.
    let (_, repair) = file_under(
        &mut conn,
        Parent::Loose,
        &[("repair jarvis", &["machine"], &[])],
    );
    assert_eq!(ask(&mut conn, 2.0)["ticket"], repair[0].as_str());
    // So is an open ticket under no epic: releasing it is the only way a person can ask for it.
    let (_, loose) = file_under(&mut conn, Parent::Loose, &[("loose", &[], &[])]);
    assert_eq!(ask(&mut conn, 2.5)["ticket"], loose[0].as_str());
    set_workers(&mut conn, &epic.unwrap(), 1);
    assert_eq!(ask(&mut conn, 3.0)["ticket"], waiting[0].as_str());
    // The epic set to 0 workers still holds its ticket back.
    assert_eq!(ask(&mut conn, 4.0), Value::Null);
    assert!(tickets::load(&conn, stopped[0].as_str()).unwrap().is_some());
}

#[test]
fn a_ticket_needing_a_person_is_not_offered() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(
        &mut conn,
        &[
            ("sign the build", &["needs:person", "machine"], &[]),
            ("anything", &[], &[]),
        ],
    );
    let (_, first) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    assert_eq!(first["ticket"], ids[1].as_str());
    let (_, second) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(1.0));
    assert_eq!(second, Value::Null);
    // A daemon that asks why gets the reason instead of `null`, for `ignite status`.
    let mut explained = free("m-1", &["local_b"]);
    explained["explain"] = json!(true);
    let (_, why) = post(&mut conn, "/next", &explained, at(2.0));
    assert_eq!(
        why,
        json!({"nothing": "no ready ticket is offered: file tickets, or release an epic"})
    );
}

#[test]
fn a_reserved_ticket_is_claimed_only_with_its_code_from_its_machine() {
    let mut conn = hub::open_in_memory().unwrap();
    let id = file(&mut conn, &[("reserved", &[], &[])]).remove(0);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    let code = reservation["code"].as_str().unwrap();
    assert_eq!(code.len(), 8);
    let mut wrong = claim_reserved(&reservation, "local_a");
    wrong["code"] = json!("00000000");
    let mut elsewhere = claim_reserved(&reservation, "local_x");
    elsewhere["machine"] = json!("m-2");
    for body in [claim(id.as_str(), "local_a"), wrong, elsewhere] {
        let (status, reply) = post(&mut conn, "/claims", &body, at(1.0));
        assert_eq!(status, 409, "{body}: {reply}");
    }
    // The hand-off moved to another worker on the same machine, which claims with the code.
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_b"),
        at(2.0),
    );
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["ticket"]["assignee"], "local_b");
}

#[test]
fn a_code_whose_reservation_lapsed_claims_nothing() {
    let mut conn = hub::open_in_memory().unwrap();
    file(&mut conn, &[("late", &[], &[])]);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    let (status, _) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(600.0),
    );
    assert_eq!(status, 409);
}

#[test]
fn a_paused_machine_gets_nothing_while_another_still_does() {
    let mut conn = hub::open_in_memory().unwrap();
    file(
        &mut conn,
        &[("a", &[], &[]), ("b", &[], &[]), ("c", &[], &[])],
    );
    let pause = |machine: Value, paused: bool| json!({"machine": machine, "paused": paused});
    assert_eq!(
        post(&mut conn, "/pause", &pause(json!("m-1"), true), start()).0,
        200
    );
    let (_, paused) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(1.0));
    assert_eq!(paused, Value::Null);
    let (_, other) = post(&mut conn, "/next", &free("m-2", &["local_b"]), at(2.0));
    assert!(other["ticket"].is_string(), "{other}");
    // A pause of every machine outlasts resuming one; resuming every machine ends both.
    post(&mut conn, "/pause", &pause(Value::Null, true), at(3.0));
    post(&mut conn, "/pause", &pause(json!("m-1"), false), at(4.0));
    let (_, still) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(5.0));
    assert_eq!(still, Value::Null);
    post(&mut conn, "/pause", &pause(Value::Null, false), at(6.0));
    let (_, resumed) = post(&mut conn, "/next", &free("m-1", &["local_a"]), at(7.0));
    assert!(resumed["ticket"].is_string(), "{resumed}");
}

#[test]
fn a_release_saves_its_branch_note_and_need_for_the_next_claim() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let id = file(&mut conn, &[("half done", &[], &[])]).remove(0);
    let (_, reservation) = post(&mut conn, "/next", &free("m-1", &["local_a"]), start());
    post(
        &mut conn,
        "/claims",
        &claim_reserved(&reservation, "local_a"),
        at(1.0),
    );
    let mut body = release(id.as_str(), "local_a");
    body["needs"] = json!("macos");
    body["branch"] = json!(format!("ignite/{id}"));
    let (status, reply) = post(&mut conn, "/release", &body, at(2.0));
    assert_eq!(status, 200, "{reply}");
    let ticket = tickets::load(&conn, id.as_str()).unwrap().unwrap();
    assert!(ticket.labels.is_empty());
    assert_eq!(ticket.needs_os, Some(Os::Macos));
    assert_eq!(
        tickets::comments(&conn, id.as_str()).unwrap()[0].text,
        "Released: blocked"
    );
    let (_, again) = post(&mut conn, "/next", &free("m-1", &["local_b"]), at(3.0));
    assert_eq!(again["ticket"], id.as_str());
    let (status, brief) = post(
        &mut conn,
        "/claims",
        &claim_reserved(&again, "local_b"),
        at(4.0),
    );
    assert_eq!(status, 200, "{brief}");
    assert_eq!(brief["saved_branch"], format!("ignite/{id}"));
    // Released for a person, it is offered to nobody.
    body["session"] = json!("local_b");
    body["needs"] = json!("person");
    post(&mut conn, "/release", &body, at(5.0));
    let ticket = tickets::load(&conn, id.as_str()).unwrap().unwrap();
    assert_eq!(ticket.labels, vec!["needs:person"]);
    let (_, none) = post(&mut conn, "/next", &free("m-1", &["local_c"]), at(6.0));
    assert_eq!(none, Value::Null);
}

#[test]
fn a_close_with_no_landing_closes_the_ticket_and_frees_the_worker() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let id = file(&mut conn, &[("obsolete", &[], &[])]).remove(0);
    let id = id.as_str();
    post(&mut conn, "/claims", &claim(id, "local_a"), start());
    let beat = json!({"machine": "m-1", "version": "d97ed0e", "workers": [
        {"session": "local_a", "app": "claude", "role": "worker", "state": "working",
         "ticket": id}]});
    assert_eq!(
        post(&mut conn, "/machines/heartbeat", &beat, at(1.0)).0,
        200
    );
    let close = |session: &str| {
        json!({"ticket": id, "machine": "m-1", "session": session,
               "reason": "obsolete: the code moved on"})
    };
    assert_eq!(post(&mut conn, "/close", &close("local_b"), at(2.0)).0, 409);
    let (status, reply) = post(&mut conn, "/close", &close("local_a"), at(3.0));
    assert_eq!(status, 200, "{reply}");
    let ticket = tickets::load(&conn, id).unwrap().unwrap();
    assert_eq!(
        ticket.status,
        ignite_core::protocol::tickets::Status::Closed
    );
    assert_eq!(
        ticket.close_reason.as_deref(),
        Some("obsolete: the code moved on")
    );
    assert!(ticket.landed.is_none());
    let (state, held): (String, Option<String>) = conn
        .query_row(
            "SELECT state, ticket FROM workers WHERE session = 'local_a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((state.as_str(), held), ("idle", None));
    // A resent close answers as the first did.
    assert_eq!(post(&mut conn, "/close", &close("local_a"), at(4.0)).0, 200);
}
