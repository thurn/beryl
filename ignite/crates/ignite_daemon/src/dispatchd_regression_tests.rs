//! A regression reaches a worker here only through a reservation the hub gave this machine, and
//! resumes the transcript of the ticket that caused it when available.

use ignite_core::protocol::hub::Reservation;
use ignite_core::protocol::tickets::{CAUSED_BY_KEY, CULPRIT_AFFINITY_KEY};
use ignite_core::traits::Clock;
use serde_json::json;

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
    assert_eq!(resume.culprit.unwrap().as_str(), "T.1");
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
    assert_eq!(resume.culprit.unwrap().as_str(), "T.1");
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

#[test]
fn a_regression_uses_attribution_from_a_later_report() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    let mut reported = regression("R", "T.1");
    reported
        .ticket
        .metadata
        .insert(CAUSED_BY_KEY.to_owned(), json!("T.2"));
    rig.hub.insert_ticket(reported);
    reserve_for(&rig, "R", "reservation-2");

    tick(&rig);

    let resumes = fake.resumes();
    let resume = resumes[0].as_ref().unwrap();
    assert_eq!(resume.culprit, Some(ticket_id("T.2")));
    assert!(resume.prompt.contains("landing of T.2"));
    assert!(resume.fresh_reason.is_none());
}

#[test]
fn a_reservation_on_another_machine_explains_why_it_starts_fresh() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    let mut reported = regression("R", "T.1");
    reported.ticket.metadata.insert(
        CULPRIT_AFFINITY_KEY.to_owned(),
        json!({
            "landing": {"machine": "m-away", "provider": "codex"},
            "machine_label": "machine:remote",
            "added_label": true,
        }),
    );
    rig.hub.insert_ticket(reported);
    reserve_for(&rig, "R", "reservation-2");

    tick(&rig);

    let resumes = fake.resumes();
    let resume = resumes[0].as_ref().unwrap();
    assert_eq!(
        resume.fresh_reason.as_deref(),
        Some("the transcript is on remote")
    );
}

#[test]
fn the_worker_already_holding_a_regression_receives_the_late_attribution() {
    use super::tell_tests::claimed;

    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    let mut unknown = regression("R", "T.1");
    unknown.ticket.body.clear();
    rig.hub.insert_ticket(unknown);
    reserve_for(&rig, "R", "reservation-1");
    tick(&rig);
    let worker = "cli-claude-00000001";
    rig.hold(worker, "R");
    tick(&rig);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();

    let attribution = "The culprit search for rust:a identified T.1 (commit 5e1b0aa).";
    let mut attributed = claimed(
        "R",
        worker,
        &[(1, "2027-01-02T00:00:00.000Z", attribution)],
        "2027-01-01T00:00:00.000Z",
    );
    attributed
        .ticket
        .metadata
        .insert(CAUSED_BY_KEY.to_owned(), json!("T.1"));
    rig.hub.insert_ticket(attributed);
    rig.hub.push_change("ticket", Some(ticket_id("R")));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();

    assert_eq!(fake.sent().len(), 1);
    assert_eq!(fake.sent()[0].session, session(worker));
    assert!(fake.sent()[0].text.contains(attribution));
}
