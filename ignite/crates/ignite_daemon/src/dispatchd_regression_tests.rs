//! A regression reaches a worker here only through a reservation the hub gave this machine, and
//! resumes the transcript of the ticket that caused it when available.

use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::cli_tests::{cli, tick};
use super::tests::{Rig, detail, rig, session, ticket_id};
use super::*;

/// A regression ticket `id` whose `caused-by` line names `cause`.
fn regression(id: &str, cause: &str) -> TicketDetail {
    let body = format!("caused-by {cause} (commit 5e1b0aa)\nfirst bad 5e1b0aa, last good 77c2d14");
    detail(id, "open", None, &["regression"], &body)
}

/// The hub reserves `ticket`, already in the fake hub, for `worker` on this machine.
fn reserve_for(rig: &Rig, ticket: &str, worker: &str) {
    rig.hub.queue_reservation(Reservation {
        provider: Some("claude".to_owned()),
        ticket: ticket_id(ticket),
        machine: rig.ctx.machine.clone(),
        session: session(worker),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    });
}

#[test]
fn the_culprit_is_chosen_once_an_unread_regression_can_be_read() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    reserve_for(&rig, "R", "reservation-2");
    tick(&rig);
    assert!(fake.begun().is_empty());
    assert!(rig.dispatcher.view().unwrap().is_empty());
    rig.hub.insert_ticket(regression("R", "T.1"));
    tick(&rig);
    assert_eq!(fake.begun()[0].0.session.as_str(), "reservation-2");
    let resume = fake.resumes()[0]
        .clone()
        .expect("culprit transcript requested");
    assert_eq!(resume.culprit.as_str(), "T.1");
    assert!(resume.prompt.contains("--reservation 7f3a"));
}

#[test]
fn a_regression_the_hub_did_not_reserve_here_reaches_no_worker() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    // Another machine found the regression, so the hub reserves it there, not here.
    rig.hub.insert_ticket(regression("R", "T.1"));
    rig.hub.push_change("ticket", Some(ticket_id("R")));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    tick(&rig);
    assert!(fake.begun().is_empty());
}

/// A reopened ticket whose landing a tip round reverted.
fn reverted(id: &str) -> TicketDetail {
    let mut ticket = detail(id, "open", None, &[], "");
    ticket.ticket.metadata.insert(
        "reverted".to_owned(),
        serde_json::json!({"revert": "abc1234", "landed": ["def5678"], "checks": ["rust:a::b"]}),
    );
    ticket
}

#[test]
fn a_reverted_ticket_requests_the_previous_landing_transcript() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    rig.hub.insert_ticket(reverted("T.1"));
    reserve_for(&rig, "T.1", "reservation-2");
    tick(&rig);
    assert_eq!(fake.begun()[0].0.session.as_str(), "reservation-2");
    let resume = fake.resumes()[0]
        .clone()
        .expect("landing transcript requested");
    assert_eq!(resume.culprit.as_str(), "T.1");
    let prompt = resume.prompt;
    assert!(
        prompt.contains(
            "your landing of T.1 was reverted: the tip did not build with it (rust:a::b). Run \
             `ignite start T.1 --reservation 7f3a`"
        ),
        "{prompt}"
    );
    assert_eq!(rig.state_of("cli-claude-00000001"), "reserved");
}
