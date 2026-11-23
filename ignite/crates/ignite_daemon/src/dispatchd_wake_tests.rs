//! Workers that end their turn waiting on an `ignite` verb are let stop and woken, and workers
//! whose ticket Ignite released may only claim a successor the daemon reserved for them.

use ignite_core::traits::Clock;

use super::restart_tests::DbFile;
use super::tests::{Rig, START, detail, lock, request, rig, rig_on, session, ticket_id};
use super::*;
use crate::server::Verb;

/// A rig whose one pool worker, `local_w1`, holds `T.1`.
fn holding() -> Rig {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    rig.hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    super::cli_tests::tick(&rig);
    rig
}

fn ended(rig: &Rig, args: &[&str]) -> String {
    let verb = TurnEndedVerb::new(rig.headless_dispatch.clone(), "jarvis".to_owned());
    let reply = verb.run(&rig.ctx, &request("turn-ended", args, None));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    reply.text
}

fn verdict(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or("")
}

/// `local_w1` started `ignite <verb>` in the background now, and is no longer waiting on it.
fn runs_in_background(rig: &Rig, verb: &str) {
    lock(&rig.host.verbs).insert(
        session("local_w1"),
        vec![RunningVerb {
            verb: verb.to_owned(),
            handle: None,
            started: rig.clock.now(),
            step: None,
        }],
    );
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: format!("{verb} x"),
            since: rig.clock.now(),
            running: false,
        },
    );
}

#[test]
fn a_worker_waiting_on_a_background_run_is_let_stop_and_never_released() {
    let rig = holding();
    runs_in_background(&rig, "run");
    let first = ended(&rig, &["--session", "local_w1"]);
    assert_eq!(verdict(&first), "pass", "{first}");
    assert!(first.contains("`ignite run` is still running"), "{first}");
    let again = ended(&rig, &["--session", "local_w1", "--again"]);
    assert_eq!(verdict(&again), "pass", "{again}");
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn the_block_text_names_the_way_to_wait() {
    let rig = holding();
    let text = ended(&rig, &["--session", "local_w1"]);
    assert_eq!(verdict(&text), "block", "{text}");
    assert!(text.contains("`ignite run -- <command>`"), "{text}");
    assert!(text.contains("you are woken when it ends"), "{text}");
    assert!(
        text.contains("only a final turn takes a ✅, ⛔ or 🔍 title"),
        "{text}"
    );
    assert!(text.contains("`ignite guide turn-end`"), "{text}");
}

#[test]
fn a_worker_released_at_turn_end_may_not_claim_again() {
    let rig = holding();
    let text = ended(&rig, &["--session", "local_w1", "--again"]);
    assert_eq!(verdict(&text), "released", "{text}");
    assert!(text.contains("Stop now"), "{text}");
    let w1 = session("local_w1");
    let t1 = ticket_id("T.1");
    let refusal = rig.headless_dispatch.refuse_start(&w1, Some(&t1)).unwrap();
    assert!(refusal.starts_with("refused"), "{refusal}");
    assert!(refusal.contains("end your turn"), "{refusal}");
    assert!(
        rig.headless_dispatch
            .refuse_start(&w1, Some(&ticket_id("T.5")))
            .is_some()
    );
    assert!(rig.headless_dispatch.refuse_start(&w1, None).is_some());
    // Another session is not affected.
    assert!(
        rig.headless_dispatch
            .refuse_start(&session("local_w2"), Some(&t1))
            .is_none()
    );
}

#[test]
fn a_restarted_daemon_still_refuses_a_released_worker() {
    let file = DbFile::new("released");
    let now = Timestamp::from_millis(START);
    let first = rig_on(&["local_w1"], file.open(), now);
    first.hold("local_w1", "T.1");
    first
        .hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    super::cli_tests::tick(&first);
    ended(&first, &["--session", "local_w1", "--again"]);
    let restarted = rig_on(&[], file.open(), now);
    let refusal = restarted
        .headless_dispatch
        .refuse_start(&session("local_w1"), Some(&ticket_id("T.1")));
    assert!(refusal.is_some());
}

/// The session's own process in the tests, as the `Stop` hook reports it with `--parent`.
const SESSION_PROCESS: &str = "4100";

fn processes_under_session(rig: &Rig, pids: &[u32]) {
    lock(&rig.host.processes).insert(4100, pids.to_vec());
}

#[test]
fn a_turn_end_with_a_live_child_process_passes() {
    let rig = holding();
    processes_under_session(&rig, &[4210, 4211]);
    for args in [
        &["--session", "local_w1", "--parent", SESSION_PROCESS][..],
        &[
            "--session",
            "local_w1",
            "--parent",
            SESSION_PROCESS,
            "--again",
        ][..],
    ] {
        let text = ended(&rig, args);
        assert_eq!(verdict(&text), "pass", "{text}");
        assert!(text.contains("4210, 4211"), "{text}");
    }
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_turn_end_with_nothing_running_is_blocked_once_then_releases_the_ticket() {
    let rig = holding();
    processes_under_session(&rig, &[]);
    let first = ended(
        &rig,
        &["--session", "local_w1", "--parent", SESSION_PROCESS],
    );
    assert_eq!(verdict(&first), "block", "{first}");
    let again = ended(
        &rig,
        &[
            "--session",
            "local_w1",
            "--parent",
            SESSION_PROCESS,
            "--again",
        ],
    );
    assert_eq!(verdict(&again), "released", "{again}");
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("local_w1"), ticket_id("T.1"))]
    );
}

#[test]
fn a_worker_that_started_a_command_after_the_block_passes_its_second_stop() {
    let rig = holding();
    processes_under_session(&rig, &[]);
    let first = ended(
        &rig,
        &["--session", "local_w1", "--parent", SESSION_PROCESS],
    );
    assert_eq!(verdict(&first), "block", "{first}");
    processes_under_session(&rig, &[4300]);
    let again = ended(
        &rig,
        &[
            "--session",
            "local_w1",
            "--parent",
            SESSION_PROCESS,
            "--again",
        ],
    );
    assert_eq!(verdict(&again), "pass", "{again}");
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn processes_that_cannot_be_read_count_as_none() {
    let rig = holding();
    let first = ended(&rig, &["--session", "local_w1", "--parent", "9999"]);
    assert_eq!(verdict(&first), "block", "{first}");
}
