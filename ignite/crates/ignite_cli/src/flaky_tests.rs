//! `ignite flaky` against a real hub on a random local port, with its database in a temporary
//! directory.

use std::time::Duration;

use clap::Parser;

use ignite_core::config::Config;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::CheckState;
use ignite_hub::server::HubServer;
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

#[test]
fn a_check_switched_off_as_flaky_can_be_listed_and_switched_back_on() {
    let dir = ignite_core::test_files::temp_dir().join(format!("ignite-cli-flaky-{}", std::process::id()));
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
