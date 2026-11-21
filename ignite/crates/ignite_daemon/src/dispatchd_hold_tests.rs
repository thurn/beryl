//! The usage hold: with a sample of a plan window at or above `[pool] usage_hold_percent`, the
//! machine asks the hub for no ordinary Claude ticket, still asks for the tickets that hold
//! `master` back, and asks normally again by itself on a lower sample or once the window resets.

use ignite_core::time::Timestamp;

use ignite_core::traits::Clock;
use ignite_store::local::{plan_usage, rows::PlanUsage};

use super::cli_tests::{cli, reserve, tick};
use super::tests::{Rig, START, rig};
use crate::clid::FakeCliPool;
use std::sync::Arc;

fn sampled(label: &str, percent: f64) -> (Rig, Arc<FakeCliPool>) {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    sample(&rig, label, percent);
    (rig, fake)
}

fn sample(rig: &Rig, label: &str, percent: f64) {
    let row = PlanUsage {
        id: 0,
        at: rig.clock.now().to_string(),
        machine: rig.ctx.machine.to_string(),
        status: "ok".to_owned(),
        label: Some(label.to_owned()),
        percent_used: Some(percent),
        resets_at: Some(
            Timestamp::from_millis(START)
                .plus_seconds(3600.0)
                .to_string(),
        ),
        note: None,
        posted: 0,
        provider: "claude".to_owned(),
    };
    let pool = rig.dispatcher.pool();
    pool.with_database(|conn| plan_usage::insert(conn, &[row], rig.clock.now()))
        .unwrap();
    super::post_usage(pool, rig.hub.as_ref(), &rig.ctx.machine, 99).unwrap();
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
    let (rig, fake) = sampled("Weekly · all models", 99.2);
    assert_eq!(
        rig.hub.metrics_imports()[0].rows[0]["holds"],
        true,
        "the dashboard is told the window holds"
    );

    // The hub answers a held machine only with a ticket that holds `master` back.
    let before = rig.hub.asked().len();
    reserve(&rig, "T.1", "cli-claude-slot-1", &["regression"]);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "T.1");
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
    let (rig, fake) = sampled("5-hour", 99.0);
    tick(&rig);
    assert!(rig.hub.asked().last().unwrap().master_only);

    rig.clock.advance(900.0);
    sample(&rig, "5-hour", 12.0);
    let before = rig.hub.asked().len();
    reserve(&rig, "TA", "cli-claude-slot-2", &[]);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "TA");
    let asked = asked_master_only(&rig, before);
    assert!(asked.contains(&false), "{asked:?}");
    assert!(status(&rig).iter().all(|line| !line.contains("usage hold")));
}

#[test]
fn the_hold_ends_by_itself_when_the_window_resets() {
    let (rig, fake) = sampled("5-hour", 100.0);
    tick(&rig);
    assert!(rig.hub.asked().last().unwrap().master_only);

    // An hour on, the window has reset, though no newer sample came in.
    rig.clock.advance(3600.0);
    let before = rig.hub.asked().len();
    reserve(&rig, "TA", "cli-claude-slot-1", &[]);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "TA");
    assert!(asked_master_only(&rig, before).contains(&false));
}

#[test]
fn a_family_window_holds_only_that_familys_tiers() {
    let (rig, _) = sampled("Weekly · Opus", 99.5);
    tick(&rig);
    let free = rig.hub.asked()[0].clone();
    assert!(!free.master_only, "the other tiers are still handed out");
    for worker in &free.idle_workers {
        assert_eq!(worker.models, ["sonnet", "light"], "{worker:?}");
    }
}
