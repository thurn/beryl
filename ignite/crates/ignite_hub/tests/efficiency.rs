//! The private efficiency API over HTTP, with synthetic fleet data and registered tokens.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;

use ignite_core::config::Config;
use ignite_hub::server::HubServer;
use ignite_store::hub::{self, tokens};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

const CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"

[hub]
url = "http://127.0.0.1:0"
project = "test"

[gate]
threshold = 0.004
cap = 400
"#;

const NOW: &str = "2026-11-24T00:00:00.000Z";
const EXPORT: &str = r#"{"export":1,"host":"first","schema":42}"#;
const TOOL_CALL: &str = r#"{"table":"tool_calls","row":{"host":"first"}}"#;
const REQUEST: &str = r#"{"table":"requests","row":{"host":"first"}}"#;
const SAMPLE: &str = r#"{"status":"ok","percent_used":27}"#;

struct Fixture {
    server: HubServer,
    dir: PathBuf,
    token: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-efficiency-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let server = HubServer {
            config: Config::parse("ignite.toml", CONFIG).unwrap(),
            listen: "127.0.0.1:0".to_owned(),
            database: dir.join("hub.sqlite"),
            wordmark: None,
        };
        let conn = hub::open(&server.database).unwrap();
        let token = tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap();
        Self { server, dir, token }
    }

    fn db(&self) -> Connection {
        hub::open(&self.server.database).unwrap()
    }

    fn get(&self, address: SocketAddr, path: &str) -> Value {
        let (status, value) = get(address, path, Some(&self.token), true);
        assert_eq!(status, 200, "{path}: {value}");
        value
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn get(address: SocketAddr, path: &str, token: Option<&str>, actor: bool) -> (u16, Value) {
    let auth = token.map_or_else(String::new, |t| format!("Authorization: Bearer {t}\r\n"));
    let actor = if actor {
        "X-Actor: efficiency-test\r\n"
    } else {
        ""
    };
    let mut stream = TcpStream::connect(address).unwrap();
    write!(
        stream,
        "GET /test/api/efficiency/{path} HTTP/1.0\r\nHost: hub\r\n{auth}{actor}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let (head, body) = reply.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert!(
        head.to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    (status, serde_json::from_str(body).unwrap())
}

fn machine(conn: &Connection, id: &str, name: &str, cores: i64) {
    conn.execute(
        "INSERT INTO machines (id, name, os, cores, memory_mb, version, last_seen)
         VALUES (?1, ?2, 'windows', ?3, 16384, 'abc', ?4)",
        params![id, name, cores, NOW],
    )
    .unwrap();
}

fn metric(conn: &Connection, machine: &str, source: &str, data: &str, at: &str) {
    conn.execute(
        "INSERT OR IGNORE INTO metrics (machine, source, data, at, kind)
         VALUES (?1, ?2, ?3, ?4, json_extract(?3, '$.table'))",
        params![machine, source, data, at],
    )
    .unwrap();
}

#[test]
fn efficiency_routes_require_machine_credentials_on_the_private_listener() {
    let fixture = Fixture::new("auth");
    let public = fixture.server.start_read_only().unwrap();
    let private = fixture.server.start().unwrap();
    for path in ["records", "metrics", "state"] {
        assert_eq!(
            get(public.address(), path, Some(&fixture.token), true).0,
            404
        );
        assert_eq!(get(private.address(), path, None, true).0, 401);
        assert_eq!(get(private.address(), path, Some("unknown"), true).0, 401);
        assert_eq!(
            get(private.address(), path, Some(&fixture.token), false).0,
            401
        );
        fixture.get(private.address(), path);
    }
    for path in [
        "records?after=-1",
        "metrics?cursor=broken",
        "metrics?limit=0",
        "records?limit=5001",
    ] {
        assert_eq!(
            get(private.address(), path, Some(&fixture.token), true).0,
            400
        );
    }
    public.stop();
    private.stop();
}

#[test]
fn efficiency_records_page_in_hub_id_order_with_machine_details() {
    let fixture = Fixture::new("records");
    let conn = fixture.db();
    machine(&conn, "m-1", "first", 16);
    machine(&conn, "m-2", "second", 8);
    for (id, machine) in [(4, "m-2"), (2, "m-1"), (8, "legacy")] {
        conn.execute(
            "INSERT INTO records (id, machine, verb, class, family, queued_s, wall_s, cpu_s,
                                  peak_mb, outcome, ignite, at)
             VALUES (?1, ?2, 'run', 'tip', 'tests', 1.5, 2.0, 3.0, 128, 'pass', 'abc', ?3)",
            params![id, machine, NOW],
        )
        .unwrap();
    }
    let private = fixture.server.start().unwrap();
    let addr = private.address();
    let page = fixture.get(addr, "records?limit=2");
    let records = page["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0],
        json!({"id": 2, "machine": "m-1", "machine_name": "first", "machine_cores": 16,
               "ticket": null, "session": null, "tree": null, "verb": "run", "class": "tip",
               "family": "tests", "queued_s": 1.5, "wall_s": 2.0, "cpu_s": 3.0,
               "instructions": null, "peak_mb": 128, "outcome": "pass", "commit_sha": null,
               "ignite": "abc", "at": NOW})
    );
    assert_eq!(records[1]["id"], 4);
    assert_eq!(records[1]["machine_name"], "second");
    assert_eq!(page["next_id"], 4);
    let page = fixture.get(addr, "records?after=4&limit=2");
    assert_eq!(page["records"].as_array().unwrap().len(), 1);
    assert_eq!(page["records"][0]["id"], 8);
    assert_eq!(page["records"][0]["machine_name"], Value::Null);
    assert_eq!(page["records"][0]["machine_cores"], Value::Null);
    assert_eq!(page["next_id"], 8);
    assert_eq!(
        fixture.get(addr, "records?after=8"),
        json!({"records": [], "next_id": 8})
    );
    private.stop();
}

#[test]
fn efficiency_metrics_resume_across_equal_arrivals_and_a_hub_restart() {
    let fixture = Fixture::new("metrics");
    let conn = fixture.db();
    machine(&conn, "m-1", "first", 16);
    machine(&conn, "m-2", "second", 8);
    metric(&conn, "m-1", "first", EXPORT, NOW);
    metric(&conn, "m-1", "first", TOOL_CALL, NOW);
    metric(&conn, "m-1", "first", REQUEST, NOW);
    let private = fixture.server.start().unwrap();
    let first = fixture.get(private.address(), "metrics?limit=1");
    assert_eq!(first["rows"].as_array().unwrap().len(), 1);
    assert_eq!(first["rows"][0]["data"], EXPORT);
    let cursor = first["next_cursor"].as_str().unwrap();
    private.stop();

    metric(&conn, "m-1", "first", TOOL_CALL, NOW);
    // A late import can have an earlier wall-clock time.
    let earlier = "2026-11-23T23:59:00.000Z";
    metric(&conn, "m-2", "plan_usage", SAMPLE, earlier);
    let private = fixture.server.start().unwrap();
    let addr = private.address();
    let page = fixture.get(addr, &format!("metrics?cursor={cursor}&limit=2"));
    assert_eq!(
        page["rows"],
        json!([
            {"machine": "m-1", "machine_name": "first", "source": "first", "data": TOOL_CALL, "at": NOW},
            {"machine": "m-1", "machine_name": "first", "source": "first", "data": REQUEST, "at": NOW}
        ])
    );
    assert_eq!(
        page["headers"],
        json!([{"machine": "m-1", "source": "first", "data": EXPORT}])
    );
    let cursor = page["next_cursor"].as_str().unwrap();
    let page = fixture.get(addr, &format!("metrics?cursor={cursor}&limit=2"));
    assert_eq!(
        page["rows"],
        json!([{"machine": "m-2", "machine_name": "second", "source": "plan_usage",
                "data": SAMPLE, "at": earlier}])
    );
    assert_eq!(page["headers"], json!([]));
    let cursor = page["next_cursor"].as_str().unwrap();
    assert_eq!(
        fixture.get(addr, &format!("metrics?cursor={cursor}")),
        json!({"rows": [], "headers": [], "next_cursor": cursor})
    );
    private.stop();
}

#[test]
fn efficiency_state_includes_report_timings_and_regression_attribution() {
    let fixture = Fixture::new("state");
    let conn = fixture.db();
    conn.execute_batch(&format!(
        "INSERT INTO tickets (id, title, created_at, updated_at, created_by, version)
         VALUES ('FIX', 'Repair tests', '{NOW}', '{NOW}', 'tester', 1),
                ('OLD', 'Previous repair', '{NOW}', '{NOW}', 'tester', 2),
                ('CAUSE', 'Change tests', '{NOW}', '{NOW}', 'tester', 3);
         INSERT INTO regressions (check_name, ticket, first_bad, last_good, state, culprit_ticket)
         VALUES ('tests:smoke', 'FIX', 'bad', 'good', 'open', 'CAUSE');
         INSERT INTO tips (commit_sha, seen, verified, state)
         VALUES ('good', '{NOW}', '{NOW}', 'verified');
         INSERT INTO handoffs (seq, machine, ticket, kind, at)
         VALUES (4, 'm-1', 'OLD', 'regression', '{NOW}');
         INSERT INTO workers (machine, session, app, role, state)
         VALUES ('m-1', 'worker-1', 'codex', 'worker', 'idle');"
    ))
    .unwrap();
    let private = fixture.server.start().unwrap();
    assert_eq!(
        fixture.get(private.address(), "state"),
        json!({
            "tickets": [["CAUSE", NOW, null, null, 0], ["FIX", NOW, null, null, 1],
                        ["OLD", NOW, null, null, 1]],
            "tips": [["good", NOW, NOW, "verified"]],
            "regressions": [{"check_name": "tests:smoke", "ticket": "FIX", "first_bad": "bad",
                             "last_good": "good", "state": "open", "platform": null,
                             "culprit_ticket": "CAUSE"}],
            "handoffs": [[4, "m-1", "OLD", "regression", NOW]],
            "workers": [["m-1", "worker-1", "worker"]]
        })
    );
    private.stop();
}
