//! The ticket verbs against a real hub on a random local port, with its database in a temporary
//! directory.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use ignite_core::config::Config;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_hub::server::{HubServer, RunningHub};
use ignite_store::hub;
use serde_json::{Value, json};

use super::http::Hub;
use super::{execute, parse};

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

struct Fixture {
    dir: PathBuf,
    running: Option<RunningHub>,
    token: String,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("ignite-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = hub::open(&dir.join("hub.sqlite")).unwrap();
        let token = hub::tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap();
        let server = HubServer {
            config: Config::parse("ignite.toml", CONFIG).unwrap(),
            listen: "127.0.0.1:0".to_owned(),
            database: dir.join("hub.sqlite"),
            wordmark: None,
        };
        let running = server.start().unwrap();
        Fixture {
            dir,
            running: Some(running),
            token,
        }
    }

    fn client(&self, token: bool) -> Hub {
        let address = self.running.as_ref().unwrap().address();
        Hub::new(
            &format!("http://{address}"),
            "pj",
            token.then(|| self.token.clone()),
            "tester".to_owned(),
            Duration::ZERO,
        )
    }

    /// Runs `ignite ticket <line>` and returns its reply.
    fn run(&self, line: &str) -> Reply {
        execute(&self.client(true), &parse(&format!("ignite ticket {line}")))
            .unwrap_or_else(super::http::Failure::into_reply)
    }

    fn json(&self, line: &str) -> Value {
        let reply = self.run(line);
        assert_eq!(reply.exit, Exit::Success, "{line}: {}", reply.text);
        reply.json.unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn tickets_can_be_created_shown_listed_linked_claimed_and_closed() {
    let hub = Fixture::new("flow");
    let epic = hub.json("create Epic --type epic --workers 3 --max-workers 5 -p P1");
    let epic_id = epic["results"][0]["id"].as_str().unwrap().to_owned();
    assert!(
        epic_id.starts_with('T'),
        "IDs print with their T: {epic_id}"
    );
    let child = hub.json(&format!(
        "create Child --parent {epic_id} --labels tier:sonnet,role:executor --cores 1.5"
    ));
    let child_id = child["results"][0]["id"].as_str().unwrap().to_owned();
    let other = hub.json(&format!("create Other --parent {epic_id}"));
    let other_id = other["results"][0]["id"].as_str().unwrap().to_owned();

    let shown = hub.json(&format!("show {epic_id}"));
    assert_eq!(
        (shown["workers"].clone(), shown["max_workers"].clone()),
        (json!(3), json!(5))
    );
    assert_eq!(shown["priority"], 1);
    assert_eq!(hub.json(&format!("show {child_id}"))["cores"], 1.5);

    hub.json(&format!("dep add {other_id} {child_id}"));
    assert_eq!(
        hub.json(&format!("show {child_id}"))["status"],
        "pending_approval",
        "a ticket is filed pending approval"
    );
    assert_eq!(
        hub.json("ready").as_array().unwrap().len(),
        0,
        "nothing is ready before a person releases it"
    );
    hub.json(&format!("update {epic_id} --release"));
    for id in [&epic_id, &child_id, &other_id] {
        assert_eq!(
            hub.json(&format!("show {id}"))["status"],
            "open",
            "releasing the epic releases {id} below it"
        );
    }
    let ready = hub.json("ready");
    let ids: Vec<&str> = ready
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&child_id.as_str()) && !ids.contains(&other_id.as_str()),
        "{ids:?}"
    );
    assert!(ready[0].get("body").is_none(), "lists leave out long text");

    assert_eq!(
        hub.json(&format!("children {epic_id}"))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(hub.json("search Child").as_array().unwrap().len(), 1);
    assert_eq!(
        hub.json("list --label tier:sonnet")
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let text = hub.run("list").text;
    assert!(
        text.contains("Child") && text.contains("[tier:sonnet, role:executor]"),
        "{text}"
    );

    hub.json(&format!("claim {child_id}"));
    hub.json(&format!("comment {child_id} Handoff:done"));
    let with_comments = hub.json(&format!("show {child_id} --comments"));
    assert_eq!(with_comments["comments"][0]["text"], "Handoff:done");
    assert_eq!(with_comments["status"], "in_progress");
    hub.json(&format!(
        "update {child_id} --add-label x --title Renamed --assignee ''"
    ));
    hub.json(&format!("close {child_id} --reason done"));
    assert_eq!(hub.json(&format!("show {child_id}"))["status"], "closed");
    let ready: Vec<String> = hub
        .json("ready")
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        ready,
        std::slice::from_ref(&other_id),
        "closing the dependency frees the dependent"
    );
    hub.json(&format!("reopen {child_id}"));
    hub.json(&format!("defer {other_id} --until 2099-01-01T00:00:00Z"));
    assert_eq!(
        hub.json(&format!("show {other_id}"))["defer_until"],
        "2099-01-01T00:00:00.000Z"
    );
    assert_eq!(hub.json("graph")["tickets"].as_array().unwrap().len(), 3);
}

#[test]
fn a_batch_file_makes_an_epic_with_three_children_and_two_edges_in_one_request() {
    let hub = Fixture::new("batch");
    let file = hub.dir.join("plan.json");
    let plan = json!({"ops": [
        {"op": "create", "fields": {"title": "Epic", "type": "epic"}},
        {"op": "create", "parent": "$0", "fields": {"title": "One"}},
        {"op": "create", "parent": "$0", "fields": {"title": "Two"}},
        {"op": "create", "parent": "$0", "fields": {"title": "Three"}},
        {"op": "dep_add", "id": "$2", "on": "$1"},
        {"op": "dep_add", "id": "$3", "on": "$2"},
    ]});
    std::fs::write(&file, plan.to_string()).unwrap();
    let before = hub.json("graph")["seq"].as_u64().unwrap();
    let reply = hub.json(&format!("batch {}", file.display()));
    assert_eq!(reply["results"].as_array().unwrap().len(), 6);
    let graph = hub.json("graph");
    assert_eq!(graph["tickets"].as_array().unwrap().len(), 4);
    assert_eq!(
        graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "blocks")
            .count(),
        2
    );
    assert!(graph["seq"].as_u64().unwrap() > before);
}

#[test]
fn a_batch_with_a_bad_operation_changes_nothing() {
    let hub = Fixture::new("atomic");
    let file = hub.dir.join("bad.json");
    let plan = json!({"ops": [
        {"op": "create", "fields": {"title": "Epic"}},
        {"op": "create", "parent": "$0", "fields": {"title": "Child"}},
        {"op": "dep_add", "id": "$1", "on": "nosuchticket"},
    ]});
    std::fs::write(&file, plan.to_string()).unwrap();
    let reply = hub.run(&format!("batch {}", file.display()));
    assert_ne!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(hub.json("list --all"), json!([]), "nothing was created");
}

#[test]
fn wait_returns_when_another_writer_closes_the_ticket() {
    let hub = Fixture::new("wait");
    let created = hub.json("create Waited");
    let id = created["results"][0]["id"].as_str().unwrap().to_owned();
    let waiting = {
        let client = hub.client(true);
        let command = parse(&format!(
            "ignite ticket wait {id} --status closed --timeout 60"
        ));
        thread::spawn(move || execute(&client, &command).unwrap())
    };
    // The wait cannot return before this write: the ticket is open until it lands.
    hub.json(&format!("close {id} --reason done"));
    let reply = waiting.join().unwrap();
    assert_eq!(reply.exit, Exit::Success);
    assert_eq!(reply.json.unwrap()["status"], "closed");
}

#[test]
fn wait_that_runs_out_exits_with_the_running_code_and_a_met_status_returns_at_once() {
    let hub = Fixture::new("timeout");
    let id = hub.json("create Open --release")["results"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = hub.run(&format!("wait {id} --status closed --timeout 0"));
    assert_eq!(reply.exit, Exit::Running);
    assert!(reply.text.contains("is open, not closed"), "{}", reply.text);
    assert_eq!(
        hub.run(&format!("wait {id} --status open --timeout 0"))
            .exit,
        Exit::Success
    );
}

#[test]
fn failures_use_the_shared_exit_codes() {
    let hub = Fixture::new("exits");
    let id = hub.json("create One")["results"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(hub.run("show nosuchticket").exit, Exit::Usage);
    assert_eq!(
        hub.run(&format!("update {id}")).exit,
        Exit::Usage,
        "nothing to change"
    );
    let command = parse(&format!("ignite ticket comment {id} hi"));
    let refused = execute(&hub.client(false), &command).unwrap_err();
    assert_eq!(
        refused.exit,
        Exit::Usage,
        "a write needs a token: {}",
        refused.message
    );
    let down = Hub::new("http://127.0.0.1:1", "pj", None, "t".into(), Duration::ZERO);
    let unreachable = execute(&down, &parse("ignite ticket list")).unwrap_err();
    assert_eq!(unreachable.exit, Exit::Unreachable);
}

#[test]
fn dep_list_multi_show_and_open_children_gather_related_tickets() {
    let hub = Fixture::new("related");
    let id = |reply: Value| reply["results"][0]["id"].as_str().unwrap().to_owned();
    let epic = id(hub.json("create Epic --type epic"));
    let first = id(hub.json(&format!("create First --parent {epic}")));
    let second = id(hub.json(&format!("create Second --parent {epic} --release")));
    let done = id(hub.json(&format!("create Done --parent {epic}")));
    hub.json(&format!("dep add {second} {first}"));
    hub.json(&format!("close {done} --reason obsolete"));
    hub.json(&format!("claim {first}"));
    hub.json(&format!("comment {first} Halfway"));

    let deps = hub.json(&format!("dep list {second}"));
    assert_eq!(deps["waits_on"][0]["id"], first.as_str());
    assert_eq!(deps["waits_on"][0]["status"], "in_progress");
    let text = hub.run(&format!("dep ls {first}")).text;
    assert!(
        text.contains(&format!("waited on by  {second}  open  Second  (blocks)")),
        "{text}"
    );
    assert!(
        hub.run(&format!("dep list {done}"))
            .text
            .ends_with("no dependencies")
    );

    let shown = hub.json(&format!("show {first} {second}"));
    assert_eq!(shown.as_array().unwrap().len(), 2);
    assert!(
        hub.json(&format!("show {first}")).is_object(),
        "one ID keeps one object"
    );
    let text = hub.run(&format!("show {second}")).text;
    assert!(
        text.contains(&format!("waits on: {first} (blocks)")),
        "{text}"
    );

    let open = hub.json(&format!("children {epic} --open"));
    let ids: Vec<&str> = open
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [first.as_str(), second.as_str()]);
    assert_eq!(open[0]["last_comment"], "Halfway");
    assert_eq!(open[1]["blocked_by"][0]["id"], first.as_str());
    let text = hub.run(&format!("children {epic} --open")).text;
    assert!(text.contains("    assignee: tester"), "{text}");
    assert!(text.contains(&format!("    blocked by: {first}")), "{text}");
    assert!(!text.contains("Done"), "{text}");
}

#[test]
fn a_rejected_command_reaches_the_hub_only_with_a_token() {
    let hub = Fixture::new("rejected");
    let command = ignite_core::protocol::hub::RejectedCommand {
        verb: "ticket dep frob".to_owned(),
        args: vec!["ticket".into(), "dep".into(), "frob".into()],
        kind: "unknown subcommand".to_owned(),
        suggestion: None,
        session: Some("s-1".parse().unwrap()),
    };
    crate::rejected::send(&hub.client(false), &command);
    crate::rejected::send(&hub.client(true), &command);
    let conn = hub::open(&hub.dir.join("hub.sqlite")).unwrap();
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT verb, machine FROM rejected_commands")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, [("ticket dep frob".to_owned(), "m-1".to_owned())]);
}
