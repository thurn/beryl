//! What `ignite turn-ended` answers a session's `Stop` hook.

use super::tests::{Rig, detail, lock, request, rig, session, ticket_id};
use super::turn::{NOTE_CHARS, REASON_CHARS, justiciar_reason, release_reason};
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
    let verb = TurnEndedVerb::new(rig.dispatcher.clone(), "jarvis".to_owned());
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
    assert!(reply.text.contains("ignite justiciar"), "{}", reply.text);
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
fn a_worker_with_a_running_test_is_waiting_and_may_stop() {
    let rig = holding(&[]);
    run(&rig, "test rust");
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
    assert_eq!(
        verdict(&ended(&rig, &["--session", "local_w1", "--again"])),
        "pass"
    );
    assert!(lock(&rig.host.releases).is_empty());
}

fn run(rig: &Rig, command: &str) {
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: command.to_owned(),
            since: ignite_core::traits::Clock::now(rig.clock.as_ref()),
            running: true,
        },
    );
}

#[test]
fn a_worker_with_a_running_land_is_never_blocked_or_released() {
    let rig = holding(&[]);
    run(&rig, "land T.1");
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
    let verb = TurnEndedVerb::new(rig.dispatcher.clone(), "jarvis".to_owned());
    let reply = verb.run(&rig.ctx, &request("turn-ended", &["--again"], None));
    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
}

fn titled(rig: &Rig, who: &str, title: &str) {
    lock(&rig.host.titles).insert(session(who), title.to_owned());
}

#[test]
fn a_worker_titled_stopped_is_released_at_once_and_a_justiciar_is_sent_for() {
    let rig = holding(&[]);
    run(&rig, "test rust");
    titled(&rig, "local_w1", "⛔ [T.1] Measure the wait");
    let args = [
        "--session",
        "local_w1",
        "--note",
        "Left the tree and ticket as they are.",
    ];
    let reply = ended(&rig, &args);
    assert_eq!(verdict(&reply), "released", "{}", reply.text);
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("local_w1"), ticket_id("T.1"))]
    );
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].host, "jarvis");
    assert!(
        reports[0]
            .details
            .contains("T.1: worker stopped unfinished: Left the tree and ticket as they are."),
        "{}",
        reports[0].details
    );
    assert!(reply.text.contains("filed"), "{}", reply.text);
}

#[test]
fn a_worker_titled_working_or_waiting_is_only_blocked() {
    for title in ["⚒️ [T.1] Measure the wait", "🔍 [T.1] Measure the wait"] {
        let rig = holding(&[]);
        titled(&rig, "local_w1", title);
        assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "block");
        assert!(lock(&rig.host.releases).is_empty());
        assert!(rig.hub.justiciar_reports().is_empty());
    }
}

#[test]
fn an_unreadable_title_changes_nothing() {
    let rig = holding(&[]);
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "block");
    assert!(lock(&rig.host.releases).is_empty());
    assert!(rig.hub.justiciar_reports().is_empty());
}

#[test]
fn a_stopped_title_leaves_people_justiciars_and_tickets_for_the_person_alone() {
    let rig = holding(&["justiciar"]);
    titled(&rig, "local_w1", "⛔ [T.1] Stuck");
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "pass");
    let rig = holding(&["needs:person"]);
    titled(&rig, "local_w1", "⛔ [T.1] Stuck");
    assert_eq!(verdict(&ended(&rig, &["--session", "local_w1"])), "block");
    rig.hold("local_person", "T.2");
    rig.hub
        .insert_ticket(detail("T.2", "in_progress", Some("local_person"), &[], ""));
    titled(&rig, "local_person", "⛔ [T.2] Stopped");
    assert_eq!(
        verdict(&ended(&rig, &["--session", "local_person"])),
        "pass"
    );
    assert!(lock(&rig.host.releases).is_empty());
    assert!(rig.hub.justiciar_reports().is_empty());
}

#[test]
fn the_justiciar_reason_quotes_the_start_of_the_last_message() {
    let ticket = ticket_id("T.1");
    let long = "y".repeat(REASON_CHARS + 10);
    let reason = justiciar_reason(&ticket, Some(&long));
    assert!(
        reason.starts_with("T.1: worker stopped unfinished: "),
        "{reason}"
    );
    assert!(reason.ends_with('…'), "{reason}");
    assert_eq!(
        justiciar_reason(&ticket, None),
        "T.1: worker stopped unfinished"
    );
}
