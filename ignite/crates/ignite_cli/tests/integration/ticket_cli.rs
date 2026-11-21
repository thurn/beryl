//! The `ignite` executable's direct hub verbs, as separate processes against a hub in this process
//! like agents run them: the hub comes from `ignite.toml` above the working directory and the
//! token from `IGNITE_TOKEN`.

use std::path::Path;
use std::process::{Command, Output};

use ignite_core::config::Config;
use ignite_hub::server::HubServer;
use ignite_store::hub;
use serde_json::Value;

fn config(address: std::net::SocketAddr) -> String {
    format!(
        r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"

[hub]
url = "http://{address}"
project = "pj"

[gate]
threshold = 0.004
cap = 400

[machine.m1]
os = "macos"
ticket_trees = 1
landing_trees = 1
trees_disk_gb = 10
floor = {{ batch = 1, tip = 1 }}
"#
    )
}

fn ignite(dir: &Path, token: &str, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ignite"));
    command
        .current_dir(dir)
        .env("IGNITE_TOKEN", token)
        .env("IGNITE_RETRY_FOR_S", "0")
        .arg("--json")
        .arg("ticket")
        .args(args);
    command
}

fn json(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

/// A hub in this process and a directory whose `ignite.toml` points at it, with a token.
fn hub_in_dir(name: &str) -> (std::path::PathBuf, String, ignite_hub::server::RunningHub) {
    let dir = ignite_core::test_files::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let conn = hub::open(&dir.join("hub.sqlite")).unwrap();
    let token = hub::tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap();
    drop(conn);
    // The server needs a valid config to start; its own address is only known afterwards.
    let placeholder = config("127.0.0.1:1".parse().unwrap());
    let running = HubServer {
        config: Config::parse("ignite.toml", &placeholder).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database: dir.join("hub.sqlite"),
        wordmark: None,
    }
    .start()
    .unwrap();
    std::fs::write(dir.join("ignite.toml"), config(running.address())).unwrap();
    (dir, token, running)
}

#[test]
fn another_process_closing_a_ticket_ends_a_waiting_process() {
    let (dir, token, running) = hub_in_dir("ignite-cli-bin");
    let created = json(
        &ignite(&dir, &token, &["create", "Waited"])
            .output()
            .unwrap(),
    );
    let id = created["results"][0]["id"].as_str().unwrap().to_owned();
    let waiting = ignite(
        &dir,
        &token,
        &["wait", &id, "--status", "closed", "--timeout", "60"],
    )
    .stdout(std::process::Stdio::piped())
    .spawn()
    .unwrap();
    // The waiting process cannot finish before this close: the ticket is open until it lands.
    let closed = ignite(&dir, &token, &["close", &id, "--reason", "done"])
        .output()
        .unwrap();
    json(&closed);
    let output = waiting.wait_with_output().unwrap();
    assert_eq!(json(&output)["status"], "closed");

    let timed_out = ignite(
        &dir,
        &token,
        &["wait", &id, "--status", "open", "--timeout", "0"],
    )
    .output()
    .unwrap();
    assert_eq!(
        timed_out.status.code(),
        Some(5),
        "a wait that runs out exits 5"
    );
    running.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_follow_up_depends_on_the_filers_unlanded_ticket_unless_independent_or_automatic() {
    let (dir, token, running) = hub_in_dir("ignite-cli-follow-up");
    let as_session = |args: &[&str]| {
        let mut command = ignite(&dir, &token, args);
        command
            .env_remove("CLAUDE_CODE_HOST_SESSION_ID")
            .env("IGNITE_SESSION", "s-filer");
        command.output().unwrap()
    };
    let held = json(&as_session(&["create", "Held"]))["results"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    json(&as_session(&["claim", &held]));

    let follow_up = json(&as_session(&["create", "Follow-up"]));
    assert_eq!(follow_up["filer_dependencies"][0]["on"], held.as_str());
    let text = ignite_cli_text(&dir, &token, &["create", "Follow-up text"]);
    assert!(text.contains(&format!("depends on {held}")), "{text}");
    assert!(text.contains("--independent"), "{text}");

    for flag in ["--independent", "--automatic"] {
        let reply = json(&as_session(&["create", "Free", flag]));
        assert!(reply.get("filer_dependencies").is_none(), "{flag}: {reply}");
    }
    running.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The text reply of `ignite ticket <args>` as the session that holds the ticket.
fn ignite_cli_text(dir: &Path, token: &str, args: &[&str]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ignite"));
    command
        .current_dir(dir)
        .env("IGNITE_TOKEN", token)
        .env("IGNITE_RETRY_FOR_S", "0")
        .env_remove("CLAUDE_CODE_HOST_SESSION_ID")
        .env("IGNITE_SESSION", "s-filer")
        .arg("ticket")
        .args(args);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn fleet_edits_are_visible_in_the_cli_and_preserve_other_fields() {
    let (dir, token, running) = hub_in_dir("ignite-cli-fleet");
    let fleet = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ignite"))
            .current_dir(&dir)
            .env("IGNITE_TOKEN", &token)
            .env("IGNITE_RETRY_FOR_S", "0")
            .arg("fleet")
            .args(args)
            .output()
            .unwrap()
    };
    let saved = json(&fleet(&["provider", "codex", "--share", "50", "--json"]));
    assert_eq!(saved["providers"]["codex"]["share"], 50);
    assert_eq!(saved["providers"]["codex"]["stop_percent"], 99);
    assert_eq!(saved["providers"]["codex"]["enabled"], true);

    let text = fleet(&[]);
    assert_eq!(text.status.code(), Some(0), "{text:?}");
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("provider codex: on, share 50, stop at 99%")
    );

    let saved = json(&fleet(&["machine", "m1", "--state", "active", "--json"]));
    assert_eq!(saved["machines"]["m1"]["disk_cap_gb"], 10);
    assert_eq!(saved["providers"]["codex"]["share"], 50);

    let saved = json(&fleet(&[
        "machine",
        "m1",
        "--state",
        "checks-only",
        "--disk-cap-gb",
        "auto",
        "--json",
    ]));
    assert_eq!(saved["machines"]["m1"]["state"], "checks-only");
    assert!(saved["machines"]["m1"]["disk_cap_gb"].is_null());

    running.stop();
    let _ = std::fs::remove_dir_all(dir);
}
