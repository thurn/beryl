//! A regression reaches a worker here only through a reservation the hub gave this machine, and
//! goes to the idle worker whose last ticket caused it when there is one.

use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::tests::{Rig, detail, handoff_to, lock, rig, session, ticket_id};
use super::*;

/// A regression ticket `id` whose `caused-by` line names `cause`.
fn regression(id: &str, cause: &str) -> TicketDetail {
    let body = format!("caused-by {cause} (commit 5e1b0aa)\nfirst bad 5e1b0aa, last good 77c2d14");
    detail(id, "open", None, &["regression"], &body)
}

/// Lets `worker` hold `ticket` and finish it, then waits until it may be handed another.
fn finished(rig: &Rig, worker: &str, ticket: &str) {
    rig.hold(worker, ticket);
    let trees = std::mem::replace(&mut *lock(&rig.host.trees), 0);
    assert!(rig.step().is_none());
    rig.finish(worker);
    assert!(rig.step().is_none());
    rig.clock.advance(SETTLE_S);
    *lock(&rig.host.trees) = trees;
}

/// The hub reserves `ticket`, already in the fake hub, for `worker` on this machine.
fn reserve_for(rig: &Rig, ticket: &str, worker: &str) {
    rig.hub.queue_reservation(Reservation {
        ticket: ticket_id(ticket),
        machine: rig.ctx.machine.clone(),
        session: session(worker),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    });
}

#[test]
fn a_regression_goes_to_the_idle_worker_whose_ticket_caused_it() {
    let rig = rig(&["local_w1", "local_w2"]);
    finished(&rig, "local_w1", "T.1");
    rig.hub.insert_ticket(regression("R", "T.1"));
    reserve_for(&rig, "R", "local_w2");
    let handoff = rig.step().unwrap();
    assert_eq!(handoff.action, DispatchAction::Handoff);
    assert_eq!(handoff.session, Some(session("local_w1")));
    // The culprit keeps the conversation in which it made the change.
    assert!(!handoff.clear);
    let prompt = handoff.prompt.unwrap();
    assert!(prompt.contains("regression caused by your last ticket"), "{prompt}");
    assert!(prompt.contains("`ignite start R --reservation 7f3a`"), "{prompt}");
    assert_eq!(rig.state_of("local_w1"), "reserved");
    assert_eq!(rig.state_of("local_w2"), "idle");
}

#[test]
fn the_culprit_is_chosen_once_an_unread_regression_can_be_read() {
    let rig = rig(&["local_w1", "local_w2"]);
    finished(&rig, "local_w1", "T.1");
    reserve_for(&rig, "R", "local_w2");
    assert!(rig.step().is_none());
    assert_eq!(rig.state_of("local_w2"), "reserved");
    rig.hub.insert_ticket(regression("R", "T.1"));
    let handoff = rig.step().unwrap();
    assert_eq!(handoff.session, Some(session("local_w1")));
    assert!(handoff.prompt.unwrap().contains("--reservation 7f3a"));
    assert_eq!(rig.state_of("local_w2"), "idle");
}

#[test]
fn a_regression_the_hub_did_not_reserve_here_reaches_no_worker() {
    let rig = rig(&["local_w1"]);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    finished(&rig, "local_w1", "T.1");
    // Another machine found the regression, so the hub reserves it there, not here.
    rig.hub.insert_ticket(regression("R", "T.1"));
    rig.hub.push_change("ticket", Some(ticket_id("R")));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    assert!(rig.step().is_none());
    assert_eq!(rig.state_of("local_w1"), "idle");
}

#[test]
fn a_regression_nobody_here_caused_goes_to_the_worker_the_hub_named() {
    let rig = rig(&["local_w1", "local_w2"]);
    finished(&rig, "local_w1", "T.1");
    rig.hub.insert_ticket(regression("R", "T.9"));
    reserve_for(&rig, "R", "local_w2");
    let handoff = rig.step().unwrap();
    assert_eq!(handoff_to(&handoff), ("local_w2", "R"));
    assert!(handoff.clear);
}

#[test]
fn a_ticket_for_the_culprit_goes_to_another_idle_worker() {
    let rig = rig(&["local_w1", "local_w2"]);
    finished(&rig, "local_w1", "T.1");
    rig.hub.insert_ticket(regression("R", "T.1"));
    rig.hub
        .insert_ticket(detail("U", "open", None, &[], ""));
    // In one request for work, the hub names `local_w2` for the regression and `local_w1` for
    // an ordinary ticket. The regression goes to `local_w1`, so the ticket goes to `local_w2`.
    reserve_for(&rig, "R", "local_w2");
    reserve_for(&rig, "U", "local_w1");
    let first = rig.step().unwrap();
    let second = rig.step().unwrap();
    assert_eq!(first.session, Some(session("local_w1")));
    assert!(first.prompt.unwrap().contains("ignite start R --reservation"));
    assert_eq!(handoff_to(&second), ("local_w2", "U"));
}
