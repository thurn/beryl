//! `ignite flaky` against a real hub on a random local port, with its database in a temporary
//! directory.

use std::time::Duration;

use clap::Parser;

use ignite_core::config::Config;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::{CheckState, FlakyReport};
use ignite_hub::server::HubServer;
use ignite_hub::upkeep::Upkeep;
use ignite_store::hub::{self, check_state, checks, rows};

use super::execute;
use crate::args::{Cli, Command};
use crate::ticket::Hub;

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

fn run(hub: &Hub, line: &str) -> Reply {
    let Command::Flaky { command } = Cli::try_parse_from(line.split_whitespace())
        .unwrap()
        .command
    else {
        panic!("not flaky: {line}");
    };
    execute(hub, &command).unwrap_or_else(crate::ticket::Failure::into_reply)
}

fn assert_evidence(hub: &Hub, ticket: &str) {
    let evidence = run(hub, &format!("ignite flaky evidence {ticket}"));
    assert_eq!(evidence.exit, Exit::Success, "{}", evidence.text);
    assert!(
        evidence
            .text
            .contains("command m-1:1\nfirst line\nlast line")
    );
    let reports = evidence.json.unwrap();
    assert_eq!(reports[0]["check"], "unit:no-hash");
    assert_eq!(reports[0]["hash"], "");
    assert_eq!(reports[1]["check"], "unit:other");
    assert_eq!(reports[2]["hash"], "a".repeat(40));
}

#[test]
fn flaky_commands_read_evidence_and_switch_checks() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-cli-flaky-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let database = dir.join("hub.sqlite");
    let mut conn = hub::open(&database).unwrap();
    let token = hub::tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap();
    checks::upsert(
        &mut conn,
        &[rows::Check {
            name: "rust:a".to_owned(),
            family: "rust".to_owned(),
            kind: "regression".to_owned(),
            state: "on".to_owned(),
            check_group: "group".to_owned(),
        }],
    )
    .unwrap();
    checks::set_state(&conn, "rust:a", CheckState::Off).unwrap();
    let now = "2026-11-17T12:00:00Z".parse().unwrap();
    let check = FlakyReport {
        check: "unit:other".parse().unwrap(),
        hash: "a".repeat(64).parse().unwrap(),
        commit: "aaaa".parse().unwrap(),
        machine: "m-1".parse().unwrap(),
        always: false,
        command: "m-1:1".to_owned(),
        output: "first line\nlast line".to_owned(),
    };
    checks::register(&conn, check.check.as_str()).unwrap();
    checks::record_flaky(&conn, &check, now).unwrap();
    let pass = Upkeep::new(None).unwrap().pass(&mut conn, now).unwrap();
    assert_eq!(pass.checks_switched_off, ["unit:other"]);
    let ticket = &pass.tickets_filed[0];
    // A ticket filed by an earlier hub can still carry imported records without command IDs.
    for (legacy, hash) in [
        ("unit:tree-hash", "a".repeat(40)),
        ("unit:no-hash", String::new()),
    ] {
        checks::register(&conn, legacy).unwrap();
        conn.execute(
            "INSERT INTO flaky (check_name, hash, commit_sha, machine, at, filed)
             VALUES (?1, ?2, 'aaaa', 'old-host', ?3, ?4)",
            [legacy, hash.as_str(), &now.to_string(), ticket],
        )
        .unwrap();
    }
    let running = HubServer {
        config: Config::parse("ignite.toml", CONFIG).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database: database.clone(),
        wordmark: None,
    }
    .start()
    .unwrap();
    let client = |token: Option<String>| {
        Hub::new(
            &format!("http://{}", running.address()),
            "pj",
            token,
            "local_abc".to_owned(),
            Duration::ZERO,
        )
    };
    let with_token = client(Some(token));

    assert_evidence(&client(None), ticket);

    let listed = run(&client(None), "ignite flaky list");
    assert_eq!(listed.exit, Exit::Success, "{}", listed.text);
    assert!(
        listed.text.starts_with("off      rust:a  flaky 0"),
        "{}",
        listed.text
    );

    let refused = run(&client(None), "ignite flaky on rust:a");
    assert_eq!(refused.exit, Exit::Usage, "{}", refused.text);

    let on = run(&with_token, "ignite flaky on rust:a -m fixed");
    assert_eq!(on.exit, Exit::Success, "{}", on.text);
    assert_eq!(on.text, "switched rust:a on (it was off)");
    assert_eq!(
        check_state::state(&conn, "rust:a").unwrap(),
        Some(CheckState::On)
    );
    let again = run(&with_token, "ignite flaky on rust:a");
    assert_eq!(again.text, "rust:a was already on");

    let listed = run(&with_token, "ignite flaky list");
    assert!(
        listed.text.contains("switched on by local_abc"),
        "{}",
        listed.text
    );
    let missing = run(&with_token, "ignite flaky off rust:nope");
    assert_ne!(missing.exit, Exit::Success, "{}", missing.text);

    running.stop();
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}
