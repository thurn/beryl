//! A shared failure, one check failing the same way for several workers here, stops ordinary
//! hand-offs, messages each running worker once, is reported once, survives a restart and ends
//! when the check passes, `green` moves or two hours pass.

use ignite_core::protocol::client::DispatchAction;
use ignite_core::traits::Clock;

use super::restart_tests::DbFile;
use super::shared::LAPSE_S;
use super::tests::{Rig, START, handoff_to, lock, rig, rig_on};
use super::*;

const CHECK: &str = "rust:alpha::tests";

/// `session`'s latest outcome of [`CHECK`], recorded `ago` seconds before the rig's clock.
fn sighting(rig: &Rig, session: &str, outcome: &str, first_line: &str, ago: f64) {
    lock(&rig.host.sightings).push(CheckSighting {
        session: session.to_owned(),
        check_name: CHECK.to_owned(),
        verb: "check".to_owned(),
        outcome: outcome.to_owned(),
        hash: None,
        first_line: (!first_line.is_empty()).then(|| first_line.to_owned()),
        tip_unknown: 0,
        at: rig.clock.now().plus_seconds(-ago).to_string(),
    });
}

/// A pool of four workers, two of them holding tickets.
fn pool() -> Rig {
    let rig = rig(&["local_w1", "local_w2", "local_w3", "local_w4"]);
    rig.hold("local_w1", "T.1");
    rig.hold("local_w2", "T.2");
    rig
}

/// The `master_only` flag of the last request for tickets.
fn last_master_only(rig: &Rig) -> bool {
    rig.hub.asked().last().expect("asked the hub").master_only
}

/// The messages given since the start, by session.
fn messages(rig: &Rig) -> Vec<(String, String)> {
    let mut given = Vec::new();
    while let Some(instruction) = rig.step() {
        if instruction.action == DispatchAction::Message {
            given.push((
                instruction.session.unwrap().to_string(),
                instruction.prompt.unwrap(),
            ));
        }
        rig.ack(instruction.id, "ok", "");
    }
    given
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

#[test]
fn two_sessions_failing_one_check_the_same_way_stop_ordinary_handoffs() {
    let rig = pool();
    sighting(
        &rig,
        "local_w1",
        "fail",
        "assertion failed: left == right",
        600.0,
    );
    sighting(
        &rig,
        "local_w2",
        "fail",
        "assertion failed: left == right",
        60.0,
    );

    let given = messages(&rig);
    assert!(last_master_only(&rig), "only tickets that hold master back");
    let sessions: Vec<&str> = given.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(sessions, ["local_w1", "local_w2"], "{given:?}");
    let text = &given[0].1;
    assert!(
        text.contains(CHECK) && text.contains("assertion failed"),
        "{text}"
    );
    assert!(text.contains("do not investigate it"), "{text}");

    // A later step sends no second message, and a worker that starts later gets its one.
    rig.hold("local_w3", "T.3");
    let later = messages(&rig);
    assert_eq!(later.len(), 1, "{later:?}");
    assert_eq!(later[0].0, "local_w3");
    assert!(messages(&rig).is_empty());

    let why = format!(
        "2 workers failed {CHECK} the same way, so only tickets that hold master back start"
    );
    assert_eq!(status(&rig)[0], why);
    assert!(rig.pool(&["list"], None).text.contains(&why));
    assert!(rig.dispatcher.idle_reason().unwrap().1.starts_with(&why));
}

#[test]
fn two_different_first_lines_are_not_a_shared_failure() {
    let rig = pool();
    sighting(
        &rig,
        "local_w1",
        "fail",
        "assertion failed: left == right",
        600.0,
    );
    sighting(
        &rig,
        "local_w2",
        "fail",
        "thread panicked at src/lib.rs:4",
        60.0,
    );
    assert!(messages(&rig).is_empty());
    assert!(!last_master_only(&rig));
    assert!(rig.dispatcher.shared_failure().is_none());
}

#[test]
fn failures_further_apart_than_fifteen_minutes_are_not_shared() {
    let rig = pool();
    sighting(&rig, "local_w1", "fail", "boom", 20.0 * 60.0);
    sighting(&rig, "local_w2", "fail", "boom", 60.0);
    assert!(messages(&rig).is_empty());
    assert!(!last_master_only(&rig));
}

#[test]
fn a_cached_failure_on_the_same_inputs_counts_as_the_same_failure() {
    let rig = pool();
    sighting(&rig, "local_w1", "fail", "boom", 120.0);
    sighting(&rig, "local_w2", "fail", "", 60.0);
    for sighting in lock(&rig.host.sightings).iter_mut() {
        sighting.hash = Some("abc".to_owned());
    }
    rig.step();
    assert!(rig.dispatcher.shared_failure().is_some());
}

#[test]
fn a_pass_of_the_check_ends_it() {
    let rig = pool();
    sighting(&rig, "local_w1", "fail", "boom", 120.0);
    sighting(&rig, "local_w2", "fail", "boom", 60.0);
    messages(&rig);
    assert!(last_master_only(&rig));

    rig.clock.advance(300.0);
    sighting(&rig, "local_w3", "pass", "", 0.0);
    rig.finish("local_w1");
    rig.clock.advance(SETTLE_S);
    messages(&rig);
    assert!(rig.dispatcher.shared_failure().is_none());
    assert!(!last_master_only(&rig));
    assert!(status(&rig).iter().all(|line| !line.contains(CHECK)));
}

#[test]
fn green_moving_ends_it_and_old_failures_do_not_start_it_again() {
    let rig = pool();
    *lock(&rig.host.green) = Some("aaa".to_owned());
    sighting(&rig, "local_w1", "fail", "boom", 120.0);
    sighting(&rig, "local_w2", "fail", "boom", 60.0);
    rig.step();
    assert!(rig.dispatcher.shared_failure().is_some());

    *lock(&rig.host.green) = Some("bbb".to_owned());
    rig.clock.advance(10.0);
    rig.step();
    assert!(rig.dispatcher.shared_failure().is_none());
    rig.clock.advance(10.0);
    rig.step();
    assert!(
        rig.dispatcher.shared_failure().is_none(),
        "seen before it ended"
    );
}

#[test]
fn it_survives_rebuilding_the_dispatcher_from_the_store() {
    let file = DbFile::new("shared");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    first.pool(
        &["add", "local_w1", "local_w2", "local_w3", "--app", "claude"],
        Some("local_d"),
    );
    first.hold("local_w1", "T.1");
    first.hold("local_w2", "T.2");
    sighting(&first, "local_w1", "fail", "boom", 120.0);
    sighting(&first, "local_w2", "fail", "boom", 60.0);
    assert_eq!(messages(&first).len(), 2);

    // The restarted daemon reads no sightings at all: the shared failure comes from the store.
    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    restarted.hold("local_w1", "T.1");
    restarted.hold("local_w2", "T.2");
    assert!(
        messages(&restarted).is_empty(),
        "each worker had its message"
    );
    assert!(last_master_only(&restarted));
    assert!(restarted.dispatcher.shared_failure().is_some());
}

#[test]
fn it_lapses_after_two_hours() {
    let rig = pool();
    sighting(&rig, "local_w1", "fail", "boom", 120.0);
    sighting(&rig, "local_w2", "fail", "boom", 60.0);
    rig.step();
    assert!(rig.dispatcher.shared_failure().is_some());

    rig.clock.advance(LAPSE_S - 60.0);
    rig.step();
    assert!(rig.dispatcher.shared_failure().is_some());
    rig.clock.advance(60.0);
    messages(&rig);
    assert!(rig.dispatcher.shared_failure().is_none());
    assert!(!last_master_only(&rig));
}

#[test]
fn a_release_after_the_tip_result_was_unknown_starts_it() {
    let rig = pool();
    sighting(&rig, "local_w1", "fail", "boom", 60.0);
    lock(&rig.host.sightings)[0].tip_unknown = 1;
    rig.step();
    assert!(
        rig.dispatcher.shared_failure().is_none(),
        "still holds its ticket"
    );

    rig.finish("local_w1");
    rig.step();
    let why = rig.dispatcher.shared_failure().expect("started");
    assert!(
        why.contains("result at the tip could not be found or run"),
        "{why}"
    );
}

#[test]
fn it_is_reported_once_unless_a_regression_names_the_check() {
    let rig = pool();
    sighting(&rig, "local_w1", "fail", "boom", 120.0);
    sighting(&rig, "local_w2", "fail", "boom", 60.0);
    rig.step();
    rig.dispatcher.report_shared(&rig.ctx, "jarvis");
    rig.dispatcher.report_shared(&rig.ctx, "jarvis");
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].subject, format!("shared:{CHECK}"));
    assert!(reports[0].on_machine);

    let named = pool();
    named.hub.insert_ticket(super::tests::detail(
        "R.1",
        "open",
        None,
        &["regression"],
        &format!("{CHECK} fails at the tip"),
    ));
    sighting(&named, "local_w1", "fail", "boom", 120.0);
    sighting(&named, "local_w2", "fail", "boom", 60.0);
    named.step();
    named.dispatcher.report_shared(&named.ctx, "jarvis");
    assert!(named.hub.justiciar_reports().is_empty());
}

#[test]
fn the_handoff_of_a_ticket_holding_master_back_still_goes_out() {
    let rig = pool();
    sighting(&rig, "local_w1", "fail", "boom", 120.0);
    sighting(&rig, "local_w2", "fail", "boom", 60.0);
    messages(&rig);
    rig.reserve("R.2", "local_w3", &["regression"]);
    let handoff = rig.step().expect("a hand-off");
    assert_eq!(handoff_to(&handoff), ("local_w3", "R.2"));
}
