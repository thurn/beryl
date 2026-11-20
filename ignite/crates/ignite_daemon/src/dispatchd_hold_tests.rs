//! The usage hold: with a sample of a plan window at or above `[pool] usage_hold_percent`, the
//! machine asks the hub for no ordinary Claude ticket, still asks for the tickets that hold
//! `master` back, and asks normally again by itself on a lower sample or once the window resets.

use ignite_core::protocol::client::DispatchAction;
use ignite_core::time::Timestamp;

use super::tests::{Rig, START, handoff_to, rig_sampling};
use super::usage::USAGE_EVERY_S;
use super::*;

/// A sample of one window, `label`, at `percent`, resetting an hour after the rig starts.
fn plan(label: &str, percent: f64) -> String {
    let resets = Timestamp::from_millis(START).plus_seconds(3600.0);
    format!(
        r#"{{"plan": {{"status": "ok", "windows": [
            {{"label": "{label}", "percentUsed": {percent}, "resetsAt": "{resets}"}}]}}}}"#
    )
}

/// A sampling dispatcher with two Claude workers that has taken the sample `raw`.
fn sampled(raw: &str) -> Rig {
    let rig = rig_sampling(
        &[],
        ignite_store::local::open_in_memory().unwrap(),
        Timestamp::from_millis(START),
        true,
    );
    for args in [
        &["add", "local_dispatcher", "--app", "claude", "--dispatcher"][..],
        &["add", "local_w1", "local_w2", "--app", "claude"][..],
    ] {
        let reply = rig.pool(args, Some("local_dispatcher"));
        assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    }
    answer(&rig, raw);
    rig
}

fn answer(rig: &Rig, raw: &str) {
    let usage = rig.step().expect("a usage instruction");
    assert_eq!(usage.action, DispatchAction::Usage);
    let reply = rig.ack(usage.id, "raw", raw);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
}

fn status(rig: &Rig) -> Vec<String> {
    use crate::doctor::StatusSection;
    crate::pool::PauseSection {
        dispatcher: rig.dispatcher.clone(),
        hub: rig.hub.clone(),
        clock: rig.clock.clone(),
        usage_hold_percent: 99,
    }
    .lines()
}

/// The `master_only` flag of each request for tickets made since `from`.
fn asked_master_only(rig: &Rig, from: usize) -> Vec<bool> {
    rig.hub.asked()[from..]
        .iter()
        .map(|free| free.master_only)
        .collect()
}

#[test]
fn a_held_machine_asks_only_for_tickets_holding_master_back() {
    let rig = sampled(&plan("Weekly · all models", 99.2));
    assert_eq!(
        rig.hub.metrics_imports()[0].rows[0]["holds"],
        true,
        "the dashboard is told the window holds"
    );

    // The hub answers a held machine only with a ticket that holds `master` back.
    let before = rig.hub.asked().len();
    rig.reserve("T.1", "local_w1", &["regression"]);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w1", "T.1"));
    let asked = asked_master_only(&rig, before);
    assert!(!asked.is_empty() && asked.iter().all(|m| *m), "{asked:?}");

    let lines = status(&rig);
    assert!(
        lines[0].starts_with("usage hold: Weekly · all models at 99%, resets"),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("master held back by T.1"), "{lines:?}");
    let usage = rig.pool(&["usage"], None).text;
    assert!(usage.starts_with("usage hold: Weekly"), "{usage}");
    assert!(
        rig.pool(&["list"], None)
            .text
            .contains("usage hold: Weekly")
    );
    let idle = rig.dispatcher.idle_reason().unwrap().1;
    assert!(idle.contains("usage hold"), "{idle}");
}

#[test]
fn the_hold_ends_by_itself_on_a_lower_sample() {
    let rig = sampled(&plan("5-hour", 99.0));
    rig.step();
    assert!(rig.hub.asked().last().unwrap().master_only);

    rig.clock.advance(USAGE_EVERY_S);
    answer(&rig, &plan("5-hour", 12.0));
    let before = rig.hub.asked().len();
    rig.reserve("TA", "local_w2", &[]);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w2", "TA"));
    let asked = asked_master_only(&rig, before);
    assert!(asked.iter().all(|m| !m), "{asked:?}");
    assert!(status(&rig).iter().all(|line| !line.contains("usage hold")));
}

#[test]
fn the_hold_ends_by_itself_when_the_window_resets() {
    let rig = sampled(&plan("5-hour", 100.0));
    rig.step();
    assert!(rig.hub.asked().last().unwrap().master_only);

    // An hour on, the window has reset, though no newer sample came in.
    rig.clock.advance(3600.0);
    let before = rig.hub.asked().len();
    rig.reserve("TA", "local_w1", &[]);
    let mut handoff = rig.step().unwrap();
    if handoff.action == DispatchAction::Usage {
        rig.ack(handoff.id, "raw", "Error: no such tool");
        handoff = rig.step().unwrap();
    }
    assert_eq!(handoff_to(&handoff), ("local_w1", "TA"));
    assert!(asked_master_only(&rig, before).iter().all(|m| !m));
}

#[test]
fn a_family_window_holds_only_that_familys_tiers() {
    let rig = sampled(&plan("Weekly · Opus", 99.5));
    rig.step();
    let free = rig.hub.asked().last().unwrap().clone();
    assert!(!free.master_only, "the other tiers are still handed out");
    for worker in &free.idle_workers {
        assert_eq!(worker.models, ["sonnet", "light"], "{worker:?}");
    }
}
