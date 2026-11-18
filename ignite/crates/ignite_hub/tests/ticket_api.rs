//! The ticket API against a real hub server on a random local port, with its database in a
//! temporary directory. Ported from the ticket service's `index.test.ts` and
//! `sqlite.integration.mjs`, with the WebSocket feed replaced by the long poll.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::thread;

use serde_json::{Value, json};

use ignite_core::config::Config;
use ignite_hub::server::{HubServer, RunningHub};
use ignite_store::hub;

const CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"

[hub]
url = "http://127.0.0.1:0"
project = "pj"

[gate]
threshold = 0.004
cap = 400

[machine.m1]
os = "macos"
ticket_trees = 1
landing_trees = 1
trees_disk_gb = 10
floor = { batch = 1, tip = 1 }
"#;

/// A temporary directory holding one hub database, with a token registered for `m-1`.
struct Fixture {
    dir: PathBuf,
    token: String,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("ignite-api-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = hub::open(&dir.join("hub.sqlite")).unwrap();
        let token = hub::tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap();
        Fixture { dir, token }
    }

    fn start(&self) -> Hub {
        let server = HubServer {
            config: Config::parse("ignite.toml", CONFIG).unwrap(),
            listen: "127.0.0.1:0".to_owned(),
            database: self.dir.join("hub.sqlite"),
            wordmark: None,
        };
        let running = server.start().unwrap();
        Hub {
            address: running.address(),
            running: Some(running),
            token: self.token.clone(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Hub {
    address: SocketAddr,
    running: Option<RunningHub>,
    token: String,
}

/// One HTTP reply.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    text: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

impl Hub {
    fn stop(&mut self) {
        self.running.take().unwrap().stop();
    }

    /// One request with the given extra header lines.
    fn send(&self, method: &str, path: &str, body: Option<&Value>, headers: &[String]) -> Reply {
        send(self.address, method, path, body, headers)
    }

    fn get(&self, path: &str) -> Reply {
        self.send("GET", &format!("/pj/api{path}"), None, &[])
    }

    /// A read that carries the machine's token, which long polls need.
    fn get_authed(&self, path: &str) -> Reply {
        self.send(
            "GET",
            &format!("/pj/api{path}"),
            None,
            &self.write_headers(),
        )
    }

    fn write_headers(&self) -> Vec<String> {
        vec![
            format!("Authorization: Bearer {}", self.token),
            "X-Actor: tester".to_owned(),
        ]
    }

    fn post_raw(&self, path: &str, body: &Value) -> Reply {
        self.send(
            "POST",
            &format!("/pj/api{path}"),
            Some(body),
            &self.write_headers(),
        )
    }

    fn post(&self, path: &str, body: &Value) -> Value {
        let reply = self.post_raw(path, body);
        assert_eq!(reply.status, 200, "{path}: {}", reply.text);
        reply.json()
    }
}

fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&Value>,
    headers: &[String],
) -> Reply {
    let payload = body.map(Value::to_string).unwrap_or_default();
    let extra: String = headers
        .iter()
        .flat_map(|line| [line.as_str(), "\r\n"])
        .collect();
    let message = format!(
        "{method} {path} HTTP/1.0\r\nHost: hub\r\n{extra}Connection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(message.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let (head, text) = reply.split_once("\r\n\r\n").unwrap();
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect();
    Reply {
        status,
        headers,
        text: text.to_owned(),
    }
}

#[test]
fn an_unknown_project_is_not_found_and_the_configured_one_serves_reads() {
    let fixture = Fixture::new("projects");
    let hub = fixture.start();
    for path in [
        "/typo-project/api/tickets",
        "/typo-project/",
        "/pjx/api/tickets",
    ] {
        assert_eq!(hub.send("GET", path, None, &[]).status, 404, "{path}");
    }
    // The root is the dashboard's home page, not part of the API.
    assert_eq!(hub.send("GET", "/", None, &[]).status, 200);
    let reply = hub.get("/tickets");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json(), json!([]));
    assert!(
        reply
            .header("Content-Type")
            .unwrap()
            .contains("application/json")
    );
}

#[test]
fn a_write_without_a_registered_token_and_an_actor_is_refused() {
    let fixture = Fixture::new("auth");
    let hub = fixture.start();
    let batch = json!({"ops": [{"op": "create", "fields": {"title": "T"}}]});
    let token = format!("Authorization: Bearer {}", hub.token);
    let refused = [
        vec![],
        vec![
            "Authorization: Bearer unknown".to_owned(),
            "X-Actor: a".to_owned(),
        ],
        vec![token.clone()],
    ];
    for headers in &refused {
        for path in ["/pj/api/batch", "/pj/api/leases/main", "/pj/api/import"] {
            let reply = hub.send("POST", path, Some(&batch), headers);
            assert_eq!(reply.status, 401, "{path} with {headers:?}");
        }
    }
    assert_eq!(hub.get("/tickets").json(), json!([]), "nothing was written");
    let accepted = hub.send("POST", "/pj/api/batch", Some(&batch), &hub.write_headers());
    assert_eq!(accepted.status, 200, "{}", accepted.text);
}

#[test]
fn tickets_comments_and_leases_survive_a_restart_and_a_request_key_stays_bound() {
    let fixture = Fixture::new("restart");
    let mut hub = fixture.start();
    let receipt = hub.post(
        "/batch",
        &json!({"ops": [
            {"op": "create", "fields": {"title": "Parent"}, "request_key": "durable-key"},
            {"op": "create", "parent": "$0", "fields": {"title": "Child"}},
            {"op": "comment", "id": "$1", "text": "Preserved comment"},
        ]}),
    );
    let parent = receipt["results"][0]["id"].as_str().unwrap().to_owned();
    let child = receipt["results"][1]["id"].as_str().unwrap().to_owned();
    let lease = hub.post(
        "/leases/main",
        &json!({"op": "acquire", "holder": "Holder", "ttl_s": 180}),
    );
    let original = hub
        .get(&format!("/tickets/{child}?comments=1&history=1"))
        .json();
    assert_eq!(original["title"], "Child");
    assert_eq!(original["comments"][0]["text"], "Preserved comment");
    hub.stop();

    let hub = fixture.start();
    let actual = hub
        .get(&format!("/tickets/{child}?comments=1&history=1"))
        .json();
    assert_eq!(actual, original);
    let leases = hub.get("/leases").json();
    assert_eq!(
        leases
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "main"),
        Some(&lease)
    );
    let again = hub.post(
        "/batch",
        &json!({"ops": [{"op": "create", "fields": {"title": "Parent"}, "request_key": "durable-key"}]}),
    );
    assert_eq!(again["results"][0]["id"], parent.as_str());
}

#[test]
fn the_feed_answers_at_once_with_changes_and_empty_when_the_wait_ends() {
    let fixture = Fixture::new("feed");
    let hub = fixture.start();
    let created = hub.post(
        "/batch",
        &json!({"ops": [{"op": "create", "fields": {"title": "A"}}]}),
    );
    let seq = created["seq"].as_u64().unwrap();
    let id = created["results"][0]["id"].as_str().unwrap().to_owned();

    let behind = hub
        .get_authed(&format!("/feed?since={}&wait=60", seq - 1))
        .json();
    assert_eq!(behind["seq"], seq);
    assert_eq!(behind["changes"][0]["ticket"], id.as_str());

    let idle = hub.get_authed(&format!("/feed?since={seq}&wait=1")).json();
    assert_eq!(idle, json!({"seq": seq, "changes": []}));

    // A long wait ends with the change as soon as the write lands. Had it waited the whole
    // minute instead, the test would still pass, but only after a minute; it cannot return
    // empty, since its wait outlasts the write.
    let address = hub.address;
    let headers = hub.write_headers();
    let waiting = thread::spawn(move || {
        let path = format!("/pj/api/feed?since={seq}&wait=60");
        send(address, "GET", &path, None, &headers).json()
    });
    hub.post(
        "/batch",
        &json!({"ops": [{"op": "update", "id": id, "set": {"title": "B"}}]}),
    );
    let woken = waiting.join().unwrap();
    assert_eq!(woken["seq"], seq + 1);
    assert_eq!(woken["changes"][0]["ticket"], id.as_str());

    let restored = hub
        .get_authed(&format!("/feed?since={}&wait=60", seq + 100))
        .json();
    assert_eq!(restored["changes"][0]["kind"], "resync");
    assert_eq!(hub.get("/feed").status, 400);
    assert_eq!(hub.get("/feed?since=x").status, 400);
}

#[test]
fn a_feed_request_without_a_token_never_waits_and_a_big_unauthenticated_body_is_refused() {
    let fixture = Fixture::new("unauth");
    let hub = fixture.start();
    // With a token this would hold a thread and a connection for a minute; without one it
    // answers at once, empty.
    let idle = hub.get("/feed?since=0&wait=60").json();
    assert_eq!(idle["changes"], json!([]));
    // The body is refused before it is read, whatever the route.
    let big = json!({"text": "x".repeat(100 * 1024)});
    let refused = hub.send("POST", "/pj/api/batch", Some(&big), &[]);
    assert_eq!(refused.status, 413);
}

#[test]
fn the_graph_answers_304_until_a_ticket_changes_and_leaves_out_long_text() {
    let fixture = Fixture::new("graph");
    let hub = fixture.start();
    let created = hub.post(
        "/batch",
        &json!({"ops": [{"op": "create", "fields": {"title": "A", "body": "long"}}]}),
    );
    let id = created["results"][0]["id"].as_str().unwrap().to_owned();
    let first = hub.get("/graph");
    assert_eq!(first.status, 200);
    let etag = first.header("ETag").unwrap().to_owned();
    let graph = first.json();
    assert_eq!(graph["tickets"][0]["id"], id.as_str());
    assert!(graph["tickets"][0].get("body").is_none(), "{graph}");
    assert_eq!(graph["tickets"][0]["ready"], true);

    let if_none_match = [format!("If-None-Match: {etag}")];
    let unchanged = hub.send("GET", "/pj/api/graph", None, &if_none_match);
    assert_eq!((unchanged.status, unchanged.text.as_str()), (304, ""));
    let all = hub.send("GET", "/pj/api/graph?all=1", None, &if_none_match);
    assert_eq!(
        all.status, 200,
        "the live and whole graphs have different tags"
    );

    hub.post(
        "/batch",
        &json!({"ops": [{"op": "update", "id": id, "set": {"title": "B"}}]}),
    );
    assert_eq!(
        hub.send("GET", "/pj/api/graph", None, &if_none_match)
            .status,
        200
    );
}

#[test]
fn the_ticket_routes_keep_the_services_statuses() {
    let fixture = Fixture::new("statuses");
    let hub = fixture.start();
    let created = hub.post(
        "/batch",
        &json!({"ops": [{"op": "create", "fields": {"title": "A"}}]}),
    );
    let id = created["results"][0]["id"].as_str().unwrap().to_owned();
    let version = hub.get(&format!("/tickets/{id}")).json()["version"].clone();

    let stale = json!({"if": [{"id": id, "version": version.as_u64().unwrap() + 5}],
                       "ops": [{"op": "close", "id": id, "reason": "done"}]});
    let conflict = hub.post_raw("/batch", &stale);
    assert_eq!(conflict.status, 409);
    assert_eq!(conflict.json()["failed"], 0);
    assert_eq!(conflict.json()["current"]["id"], id.as_str());

    let missing = json!({"ops": [{"op": "close", "id": "zz9", "reason": "done"}]});
    assert_eq!(hub.post_raw("/batch", &missing).status, 404);
    assert_eq!(hub.post_raw("/batch", &json!({"ops": "nope"})).status, 400);
    assert_eq!(hub.get("/tickets/zz9").status, 404);
    assert_eq!(hub.get("/tickets?ready=yes").status, 400);
    assert_eq!(hub.get("/tickets?status=unknown").status, 400);
    assert_eq!(hub.get("/tickets?limit=-1").status, 400);
    assert_eq!(hub.send("GET", "/pj/api/batch", None, &[]).status, 405);

    let ready = hub.get("/tickets?ready=1&status=open&limit=5").json();
    assert_eq!(ready[0]["id"], id.as_str());
    assert_eq!(hub.get("/tickets?label=missing").json(), json!([]));

    let acquire = json!({"op": "acquire", "holder": "first", "ttl_s": 60, "no_queue": true});
    hub.post("/leases/main", &acquire);
    let busy = hub.post_raw(
        "/leases/main",
        &json!({"op": "acquire", "holder": "second", "ttl_s": 60, "no_queue": true}),
    );
    assert_eq!(busy.status, 409);
    assert_eq!(busy.json()["holder"], "first");
}

#[test]
fn an_export_imports_into_an_empty_hub_once() {
    let source_fixture = Fixture::new("export");
    let source = source_fixture.start();
    let receipt = source.post(
        "/batch",
        &json!({"ops": [
            {"op": "create", "fields": {"title": "Epic", "type": "epic"}},
            {"op": "create", "parent": "$0", "fields": {"title": "Child", "labels": ["x"]}},
            {"op": "create", "parent": "$0", "fields": {"title": "Next"}, "deps": [{"on": "$1"}]},
            {"op": "comment", "id": "$1", "text": "note"},
        ]}),
    );
    let child = receipt["results"][1]["id"].as_str().unwrap().to_owned();
    let export = source.get("/export");
    assert_eq!(export.header("Content-Type"), Some("application/x-ndjson"));
    let tickets: Vec<Value> = export
        .text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(tickets.len(), 3);
    let last = tickets[2]["id"].as_str().unwrap();
    assert_eq!(source.get(&format!("/export?after={last}")).text, "");

    let target_fixture = Fixture::new("import");
    let target = target_fixture.start();
    let imported = target.post("/import", &json!({"tickets": tickets}));
    assert_eq!(imported["imported"], 3);
    let shown = |hub: &Hub| {
        let mut ticket = hub
            .get(&format!("/tickets/{child}?comments=1&history=1"))
            .json();
        ticket.as_object_mut().unwrap().remove("version");
        ticket
    };
    assert_eq!(shown(&target), shown(&source));
    assert_eq!(
        target.post("/import", &json!({"tickets": tickets}))["imported"],
        0
    );

    let mut orphan = tickets[1].clone();
    orphan["parent"] = json!("nope");
    orphan["updated_at"] = json!("2999-01-01T00:00:00.000Z");
    let refused = target.post_raw("/import", &json!({"tickets": [orphan]}));
    assert_eq!(refused.status, 400, "{}", refused.text);
    assert_eq!(
        shown(&target),
        shown(&source),
        "a refused page changes nothing"
    );
}
