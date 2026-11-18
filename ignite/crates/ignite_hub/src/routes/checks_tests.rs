//! Drives each route over HTTP: a `tiny_http` server on a local port answers with [`handle`], on a
//! database file in a temporary directory, and the tests talk to it through a plain TCP socket.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::Connection;
use serde_json::{Value, json};
use tiny_http::{Response as HttpResponse, Server};

use ignite_core::ids::MachineId;
use ignite_core::time::Timestamp;
use ignite_store::hub;

use super::{Request, handle};
use crate::auth::Writer;

fn at(seconds: f64) -> Timestamp {
    Timestamp::from_millis(0).plus_seconds(seconds)
}

/// A hub serving `handle` on a local port, with its database in a temporary directory that is
/// removed when the test ends.
struct TestHub {
    port: u16,
    conn: Arc<Mutex<Connection>>,
    dir: PathBuf,
}

impl TestHub {
    fn start(name: &str) -> TestHub {
        let dir = std::env::temp_dir().join(format!("ignite-checks-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = Arc::new(Mutex::new(hub::open(&dir.join("hub.sqlite")).unwrap()));
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let for_thread = Arc::clone(&conn);
        thread::spawn(move || {
            for mut http in server.incoming_requests() {
                let url = http.url().to_owned();
                let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
                let mut body = Vec::new();
                http.as_reader().read_to_end(&mut body).unwrap();
                let authorized = http
                    .headers()
                    .iter()
                    .any(|header| header.field.equiv("Authorization"));
                let request = Request {
                    method: http.method().as_str().to_owned(),
                    path: path.to_owned(),
                    query: query
                        .split('&')
                        .filter_map(|pair| pair.split_once('='))
                        .map(|(key, value)| (key.to_owned(), value.to_owned()))
                        .collect(),
                    writer: authorized.then(|| Writer {
                        machine: MachineId::new("m-1").unwrap(),
                        actor: "tester".to_owned(),
                    }),
                    body,
                    headers: Vec::new(),
                };
                let response = handle(&mut for_thread.lock().unwrap(), &request, at(100.0));
                let reply = response.unwrap_or_else(|| super::Response::json(404, &json!({})));
                http.respond(HttpResponse::from_data(reply.body).with_status_code(reply.status))
                    .unwrap();
            }
        });
        TestHub { port, conn, dir }
    }

    /// One HTTP request; returns the status and the decoded JSON body.
    fn send(&self, method: &str, path: &str, body: Option<&Value>, token: bool) -> (u16, Value) {
        let payload = body.map(Value::to_string).unwrap_or_default();
        let auth = if token {
            "Authorization: Bearer t\r\n"
        } else {
            ""
        };
        let message = format!(
            "{method} {path} HTTP/1.0\r\nHost: hub\r\n{auth}Connection: close\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
            payload.len()
        );
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.write_all(message.as_bytes()).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        let (head, text) = reply.split_once("\r\n\r\n").unwrap();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str(text).unwrap_or(Value::Null))
    }

    fn post(&self, path: &str, body: &Value) -> Value {
        let (status, reply) = self.send("POST", path, Some(body), true);
        assert_eq!(status, 200, "{path}: {reply}");
        reply
    }
}

impl Drop for TestHub {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn digest(n: usize) -> String {
    format!("{n:064x}")
}

fn key(check: &str, n: usize, platform: &Value) -> Value {
    json!({"check": check, "hash": digest(n), "platform": platform})
}

fn new_result(check: &str, n: usize, outcome: &str, record: u64) -> Value {
    json!({"key": key(check, n, &Value::Null), "outcome": outcome, "machine": "m-1",
           "record": record, "output": null})
}

fn record(wall_s: f64, instructions: &Value) -> Value {
    json!({"machine": "m-1", "ticket": null, "session": null, "tree": null, "verb": "check",
           "class": "agent", "family": "rust", "queued_s": 0.0, "wall_s": wall_s, "cpu_s": 3.0,
           "instructions": instructions, "peak_mb": 100, "outcome": "pass", "commit": "abcd",
           "ignite": "d97ed0e", "at": "2026-11-17T14:12:00.000Z"})
}

#[test]
fn a_lookup_of_five_thousand_keys_is_answered_in_order_in_one_request() {
    let hub = TestHub::start("lookup");
    let stored: Vec<Value> = (0..5000)
        .filter(|n| n % 2 == 0)
        .map(|n| new_result("rust:a", n, if n % 4 == 0 { "pass" } else { "fail" }, 1))
        .collect();
    hub.post("/results/store", &json!({"results": stored}));

    let keys: Vec<Value> = (0..5000).map(|n| key("rust:a", n, &Value::Null)).collect();
    let reply = hub.post("/results/lookup", &json!({"keys": keys}));
    let results = reply["results"]
        .as_array()
        .unwrap_or_else(|| panic!("{reply}"));
    assert_eq!(results.len(), 5000);
    for (n, result) in results.iter().enumerate() {
        match n % 4 {
            0 => assert_eq!(result["outcome"], "pass", "key {n}"),
            2 => assert_eq!(result["outcome"], "fail", "key {n}"),
            _ => assert!(result.is_null(), "key {n}"),
        }
        if !result.is_null() {
            assert_eq!(result["key"]["hash"], digest(n));
        }
    }
}

#[test]
fn a_platform_result_is_found_only_by_a_lookup_that_names_its_platform() {
    let hub = TestHub::start("platform");
    let store = json!({"results": [{"key": key("ui:b", 1, &json!("macos")), "outcome": "pass",
                                    "machine": "m-1", "record": 1, "output": null}]});
    hub.post("/results/store", &store);
    let keys = json!({"keys": [key("ui:b", 1, &json!("macos")), key("ui:b", 1, &Value::Null)]});
    let reply = hub.post("/results/lookup", &keys);
    assert_eq!(reply["results"][0]["outcome"], "pass");
    assert!(reply["results"][1].is_null());
}

fn list_check(hub: &TestHub, name: &str, family: &str) {
    hub.conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO checks (name, family, kind, check_group) VALUES (?1, ?2, 'regression', 'g')",
            [name, family],
        )
        .unwrap();
}

#[test]
fn storing_results_counts_each_run_and_its_cost_once_however_often_it_is_sent() {
    let hub = TestHub::start("stats");
    list_check(&hub, "rust:a", "rust");
    list_check(&hub, "rust:b", "rust");
    let ids = hub.post(
        "/records",
        &json!({"records": [record(1.0, &json!(4.0e9)), record(2.0, &Value::Null)]}),
    );
    let (first, second) = (
        ids["ids"][0].as_u64().unwrap(),
        ids["ids"][1].as_u64().unwrap(),
    );
    let store = json!({
        "results": [new_result("rust:a", 1, "pass", first), new_result("rust:b", 2, "fail", second),
                    new_result("rust:b", 3, "incomplete", second)],
        "areas": ["kernel", "ui"],
    });
    hub.post("/results/store", &store);
    hub.post("/results/store", &store);

    let (status, reply) = hub.send("GET", "/checks/stats?family=rust", None, false);
    assert_eq!(status, 200);
    let checks = reply["checks"].as_array().unwrap();
    let a = &checks[0];
    assert_eq!(a["check"], "rust:a");
    assert_eq!(
        a["areas"],
        json!([
            {"area": "kernel", "runs": 1, "failures": 0}, {"area": "ui", "runs": 1, "failures": 0}
        ])
    );
    assert_eq!(
        (a["median_cost"].as_f64(), a["unit"].as_str()),
        (Some(4.0), Some("instructions"))
    );
    let b = &checks[1];
    assert_eq!(
        b["areas"][0],
        json!({"area": "kernel", "runs": 1, "failures": 1})
    );
    assert_eq!(
        (b["median_cost"].as_f64(), b["unit"].as_str()),
        (Some(3.0), Some("cpu_s"))
    );
    // Two runs over four area counts, one of them a failure per area of check b.
    assert_eq!(reply["family_rate"].as_f64(), Some(0.5));
}

#[test]
fn a_record_batch_sent_twice_is_stored_once_with_the_same_ids() {
    let hub = TestHub::start("records");
    let batch = json!({"records": [record(1.0, &Value::Null), record(2.0, &json!(1.0e9))]});
    let first = hub.post("/records", &batch);
    let second = hub.post("/records", &batch);
    assert_eq!(first, second);
    assert_eq!(first["ids"].as_array().unwrap().len(), 2);
    let count: i64 = hub
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn a_second_flaky_report_on_another_commit_is_kept_for_upkeep_to_act_on() {
    let hub = TestHub::start("flaky");
    for commit in ["abcd", "abcd", "ef01"] {
        let report =
            json!({"check": "rust:a", "hash": digest(1), "commit": commit, "machine": "m-1"});
        hub.post("/flaky", &report);
    }
    let conn = hub.conn.lock().unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM flaky", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 2, "the same report twice is one row");
    assert!(ignite_store::hub::checks::flaky_repeated(&conn, "rust:a").unwrap());
}

#[test]
fn a_tip_batch_is_granted_to_one_machine_at_a_time() {
    let hub = TestHub::start("tips");
    let claim =
        |machine: &str| json!({"commit": "abcd", "family": "rust", "batch": 0, "machine": machine});
    assert_eq!(hub.post("/tips/claim", &claim("m-1"))["granted"], true);
    assert_eq!(hub.post("/tips/claim", &claim("m-2"))["granted"], false);
    hub.post(
        "/tips/verified",
        &json!({"commit": "abcd", "machine": "m-1", "state": "verified"}),
    );
    let state: String = hub
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT state FROM tips WHERE commit_sha = 'abcd'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "verified");
}

#[test]
fn a_regression_that_is_already_open_returns_its_ticket_and_changes_nothing() {
    let hub = TestHub::start("regression");
    hub.conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
             VALUES ('rust:a', 'tk.7', 'abcd', '0000', 'open')",
            [],
        )
        .unwrap();
    let report = json!({"check": "rust:a", "first_bad": "ef01", "last_good": "abcd",
                        "caused_by": null, "output": "boom"});
    assert_eq!(hub.post("/regressions", &report), json!({"ticket": "tk.7"}));
    let first_bad: String = hub
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT first_bad FROM regressions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(first_bad, "abcd");
}

#[test]
fn metrics_rows_are_stored_once_per_machine_and_source() {
    let hub = TestHub::start("metrics");
    let rows = json!({"machine": "m-1", "source": "claude",
                      "rows": [{"day": "2026-11-17", "usd": 1.5}, {"day": "2026-11-18", "usd": 2}]});
    assert_eq!(hub.post("/metrics/import", &rows)["stored"], 2);
    assert_eq!(hub.post("/metrics/import", &rows)["stored"], 0);
}

#[test]
fn writes_need_a_writer_and_every_route_checks_its_method_and_body() {
    let hub = TestHub::start("refusals");
    let body = json!({"records": []});
    assert_eq!(hub.send("POST", "/records", Some(&body), false).0, 401);
    assert_eq!(hub.send("GET", "/records", None, true).0, 405);
    assert_eq!(hub.send("POST", "/checks/stats", Some(&body), true).0, 405);
    assert_eq!(
        hub.send("POST", "/flaky", Some(&json!({"check": 3})), true)
            .0,
        400
    );
    assert_eq!(hub.send("GET", "/checks/stats", None, false).0, 400);
    // A path this module does not own is another module's.
    assert_eq!(hub.send("GET", "/tickets", None, false).0, 404);
    let mut conn = hub.conn.lock().unwrap();
    let request = Request {
        method: "GET".to_owned(),
        path: "/tickets".to_owned(),
        query: Vec::new(),
        writer: None,
        body: Vec::new(),
        headers: Vec::new(),
    };
    assert!(handle(&mut conn, &request, at(0.0)).is_none());
}
