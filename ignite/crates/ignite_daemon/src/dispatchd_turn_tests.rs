//! What `ignite turn-ended` answers a session's `Stop` hook.

use super::tests::{Rig, detail, lock, request, rig, session, ticket_id};
use super::turn::{NOTE_CHARS, release_reason};
use super::*;
use crate::server::Verb;

/// A rig whose one pool worker, `local_w1`, holds `T.1`, a ticket labelled `labels`.
fn holding(labels: &[&str]) -> Rig {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    rig.hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), labels, ""));
    rig
}

fn ended(rig: &Rig, args: &[&str]) -> Reply {
    let verb = TurnEndedVerb::new(rig.dispatcher.clone());
    let reply = verb.run(&rig.ctx, &request("turn-ended", args, None));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    reply
}

fn verdict(reply: &Reply) -> &str {
    reply.text.split_whitespace().next().unwrap_or("")
}

#[test]
fn a_worker_that_stops_holding_its_ticket_is_told_once_to_land_or_release() {
    let rig = holding(&[]);
    let reply = ended(&rig, &["--session", "local_w1"]);
    assert_eq!(verdict(&reply), "block", "{}", reply.text);
    assert!(reply.text.contains("T.1"), "{}", reply.text);
    assert!(reply.text.contains("ignite land -m"), "{}", reply.text);
    assert!(reply.text.contains("ignite release -m"), "{}", reply.text);
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_worker_that_stops_again_has_its_ticket_released_with_its_last_message() {
    let rig = holding(&[]);
    let args = [
        "--session",
        "local_w1",
        "--again",
        "--note",
        "Blocked by T.9's failing check.",
    ];
    let reply = ended(&rig, &args);
    assert_eq!(verdict(&reply), "released", "{}", reply.text);
    assert!(reply.text.contains("ignite/T.1"), "{}", reply.text);
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("local_w1"), ticket_id("T.1"))]
    );
    assert_eq!(
        release_reason(Some("Blocked by T.9's failing check.")),
        "The worker ended its turn twice holding the ticket without landing or releasing it. \
         Its last message: Blocked by T.9's failing check."
    );
}

#[test]
fn a_persons_session_is_never_blocked_or_released() {
    let rig = holding(&[]);
    // A person's own session holding a ticket through `/ignite` is no pool member.
    rig.hold("local_person", "T.2");
    rig.hub
        .insert_ticket(detail("T.2", "in_progress", Some("local_person"), &[], ""));
    for args in [
        &["--session", "local_person"][..],
        &["--session", "local_person", "--again"][..],
    ] {
        assert_eq!(verdict(&ended(&rig, args)), "pass");
    }
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_justiciars_session_is_never_blocked_or_released() {
    let rig = holding(&["justiciar"]);
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
    assert_eq!(
        verdict(&ended(&rig, &["--session", "local_w1", "--again"])),
        "pass"
    );
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_worker_with_a_running_command_is_never_blocked_or_released() {
    let rig = holding(&[]);
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: "land".to_owned(),
            since: ignite_core::traits::Clock::now(rig.clock.as_ref()),
            running: true,
        },
    );
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
    assert_eq!(
        verdict(&ended(&rig, &["--session", "local_w1", "--again"])),
        "pass"
    );
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn a_worker_holding_no_ticket_may_stop() {
    let rig = rig(&["local_w1"]);
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
}

#[test]
fn a_ticket_that_cannot_be_read_lets_the_worker_stop() {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
}

#[test]
fn a_long_last_message_is_cut_in_the_release_note() {
    let long = "x".repeat(NOTE_CHARS + 10);
    let reason = release_reason(Some(&long));
    assert!(reason.ends_with('…'), "{reason}");
    assert!(reason.len() < long.len() + 200);
    assert!(!release_reason(Some("  ")).contains("last message"));
}

#[test]
fn the_verb_needs_a_session() {
    let rig = holding(&[]);
    let verb = TurnEndedVerb::new(rig.dispatcher.clone());
    let reply = verb.run(&rig.ctx, &request("turn-ended", &["--again"], None));
    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
}
