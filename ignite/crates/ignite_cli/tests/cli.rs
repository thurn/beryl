//! The `ignite` executable, run as a user would run it.

use std::process::Command;

fn ignite(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(args)
        .env_remove("IGNITE_SESSION")
        .output()
        .unwrap()
}

#[test]
fn help_lists_the_agent_verbs() {
    let output = ignite(&["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for verb in [
        "start", "test", "check", "sync", "land", "run", "ticket", "why", "status",
    ] {
        assert!(help.contains(verb), "{verb} missing from {help}");
    }
}

#[test]
fn an_unbuilt_verb_names_its_ticket_and_exits_1() {
    let output = ignite(&["land", "-m", "note"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "not implemented: Landing\n"
    );
}

#[test]
fn a_usage_error_exits_nonzero() {
    let output = ignite(&["land"]);
    assert!(!output.status.success());
}
