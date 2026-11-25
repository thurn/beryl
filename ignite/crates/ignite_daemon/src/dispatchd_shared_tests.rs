//! A shared failure, one check failing the same way for several workers here, stops ordinary
//! hand-offs, messages each running worker once, is reported once, survives a restart and ends
//! when the check passes, `green` moves or two hours pass.

use ignite_core::ids::Commit;
use ignite_core::protocol::hub::OpenRegression;
use ignite_core::traits::Clock;

use super::cli_tests::{cli, reserve, tick};
use super::restart_tests::DbFile;
use super::shared::LAPSE_S;
use super::tests::{Rig, START, lock, rig, rig_on};
use super::*;
use crate::clid::FakeCliPool;

const CHECK: &str = "rust:alpha::tests";

/// `session`'s latest outcome of [`CHECK`], recorded `ago` seconds before the rig's clock.
fn sighting(rig: &Rig, session: &str, outcome: &str, first_line: &str, ago: f64) {
    let head = lock(&rig.host.green).clone();
    sighting_on(rig, session, outcome, first_line, ago, head.as_deref());
}

fn sighting_on(
    rig: &Rig,
    session: &str,
    outcome: &str,
    first_line: &str,
    ago: f64,
    head: Option<&str>,
) {
    lock(&rig.host.sightings).push(CheckSighting {
        session: session.to_owned(),
        check_name: CHECK.to_owned(),
        verb: "check".to_owned(),
        outcome: outcome.to_owned(),
        hash: None,
        first_line: (!first_line.is_empty()).then(|| first_line.to_owned()),
        tip_unknown: 0,
        at: rig.clock.now().plus_seconds(-ago).to_string(),
        head: head.map(str::to_owned),
    });
}

/// A pool of four workers, two of them holding tickets.
fn pool() -> (Rig, Arc<FakeCliPool>) {
    let rig = rig(&["cli-claude-00000001", "cli-claude-00000002"]);
    let fake = cli(&rig, 4);
    rig.hold("cli-claude-00000001", "T.1");
    rig.hold("cli-claude-00000002", "T.2");
    (rig, fake)
}

/// The first request after a tick is for ordinary slots. The extra slot is always master-only.
fn master_only(rig: &Rig, fake: &FakeCliPool) -> bool {
    let before = rig.hub.asked().len();
    let _ = messages(rig, fake);
    let asked = rig.hub.asked();
    assert!(asked.len() > before, "asked the hub");
    asked[before..].iter().all(|request| request.master_only)
}

fn messages(rig: &Rig, fake: &FakeCliPool) -> Vec<(String, String)> {
    let before = fake.sent().len();
    tick(rig);
    fake.sent()[before..]
        .iter()
        .map(|message| (message.session.to_string(), message.text.clone()))
        .collect()
}

fn status(rig: &Rig) -> Vec<String> {
    use crate::doctor::StatusSection;
    crate::pool::PauseSection {
        headless_dispatch: rig.headless_dispatch.clone(),
        hub: rig.hub.clone(),
        clock: rig.clock.clone(),
    }
    .lines()
}

#[test]
fn two_sessions_failing_one_check_the_same_way_stop_ordinary_handoffs() {
    let (rig, fake) = pool();
    sighting(
        &rig,
        "cli-claude-00000001",
        "fail",
        "assertion failed: left == right",
        600.0,
    );
    sighting(
        &rig,
        "cli-claude-00000002",
        "fail",
        "assertion failed: left == right",
        60.0,
    );

    let given = messages(&rig, &fake);
    assert!(
        master_only(&rig, &fake),
        "only tickets that hold master back"
    );
    let sessions: Vec<&str> = given.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(
        sessions,
        ["cli-claude-00000001", "cli-claude-00000002"],
        "{given:?}"
    );
    let text = &given[0].1;
    assert!(
        text.contains(CHECK) && text.contains("assertion failed"),
        "{text}"
    );
    assert!(text.contains("do not investigate it"), "{text}");

    // A later step sends no second message, and a worker that starts later gets its one.
    rig.hold("cli-claude-00000003", "T.3");
    let later = messages(&rig, &fake);
    assert_eq!(later.len(), 1, "{later:?}");
    assert_eq!(later[0].0, "cli-claude-00000003");
    assert!(messages(&rig, &fake).is_empty());

    let why = format!(
        "2 workers failed {CHECK} the same way, so only tickets that hold master back start"
    );
    assert_eq!(status(&rig)[0], why);
    assert!(rig.pool(&["list"], None).text.contains(&why));
    assert!(
        rig.headless_dispatch
            .idle_reason()
            .unwrap()
            .1
            .starts_with(&why)
    );
}

#[test]
fn two_different_first_lines_are_not_a_shared_failure() {
    let (rig, fake) = pool();
    sighting(
        &rig,
        "cli-claude-00000001",
        "fail",
        "assertion failed: left == right",
        600.0,
    );
    sighting(
        &rig,
        "cli-claude-00000002",
        "fail",
        "thread panicked at src/lib.rs:4",
        60.0,
    );
    assert!(messages(&rig, &fake).is_empty());
    assert!(!master_only(&rig, &fake));
    assert!(rig.headless_dispatch.shared_failure().is_none());
}

#[test]
fn failures_further_apart_than_fifteen_minutes_are_not_shared() {
    let (rig, fake) = pool();
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 20.0 * 60.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    assert!(messages(&rig, &fake).is_empty());
    assert!(!master_only(&rig, &fake));
}

#[test]
fn a_cached_failure_on_the_same_inputs_counts_as_the_same_failure() {
    let (rig, _) = pool();
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "", 60.0);
    for sighting in lock(&rig.host.sightings).iter_mut() {
        sighting.hash = Some("abc".to_owned());
    }
    tick(&rig);
    assert!(rig.headless_dispatch.shared_failure().is_some());
}

#[test]
fn a_pass_of_the_check_ends_it() {
    let (rig, fake) = pool();
    *lock(&rig.host.green) = Some("bbbb".to_owned());
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    messages(&rig, &fake);
    assert!(master_only(&rig, &fake));

    rig.clock.advance(300.0);
    sighting(&rig, "cli-claude-00000003", "pass", "", 0.0);
    rig.finish("cli-claude-00000001");
    messages(&rig, &fake);
    assert!(rig.headless_dispatch.shared_failure().is_none());
    assert!(!master_only(&rig, &fake));
    assert!(status(&rig).iter().all(|line| !line.contains(CHECK)));
}

#[test]
fn a_historical_pass_does_not_hide_current_shared_failures() {
    let (rig, fake) = pool();
    let older = Commit::new("aaaa").unwrap();
    let green = Commit::new("bbbb").unwrap();
    rig.git.set_not_ancestor(&green, &older);
    *lock(&rig.host.green) = Some(green.to_string());
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    sighting_on(&rig, "cli-claude-00000003", "pass", "", 0.0, Some("aaaa"));

    assert!(master_only(&rig, &fake));
}

#[test]
fn a_historical_pass_leaves_the_hold_until_a_descendant_passes() {
    let (rig, fake) = pool();
    let older = Commit::new("aaaa").unwrap();
    let green = Commit::new("bbbb").unwrap();
    rig.git.set_not_ancestor(&green, &older);
    *lock(&rig.host.green) = Some(green.to_string());
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    assert!(master_only(&rig, &fake));

    rig.clock.advance(60.0);
    sighting_on(&rig, "cli-claude-00000003", "pass", "", 0.0, Some("aaaa"));
    assert!(master_only(&rig, &fake));

    rig.clock.advance(60.0);
    sighting_on(&rig, "cli-claude-00000004", "pass", "", 0.0, Some("cccc"));
    assert!(!master_only(&rig, &fake));
}

#[test]
fn green_moving_ends_it_and_old_failures_do_not_start_it_again() {
    let (rig, _) = pool();
    *lock(&rig.host.green) = Some("aaaa".to_owned());
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    tick(&rig);
    assert!(rig.headless_dispatch.shared_failure().is_some());

    *lock(&rig.host.green) = Some("bbbb".to_owned());
    rig.clock.advance(10.0);
    tick(&rig);
    assert!(rig.headless_dispatch.shared_failure().is_none());
    rig.clock.advance(10.0);
    tick(&rig);
    assert!(
        rig.headless_dispatch.shared_failure().is_none(),
        "seen before it ended"
    );
}

#[test]
fn failures_reported_late_from_older_trees_leave_ordinary_dispatch_available() {
    let (rig, fake) = pool();
    let older = Commit::new("aaaa").unwrap();
    let newer = Commit::new("bbbb").unwrap();
    rig.git.set_not_ancestor(&newer, &older);
    *lock(&rig.host.green) = Some(older.to_string());
    tick(&rig);

    *lock(&rig.host.green) = Some(newer.to_string());
    tick(&rig);
    rig.clock.advance(120.0);
    sighting_on(
        &rig,
        "cli-claude-00000001",
        "fail",
        "boom",
        60.0,
        Some("aaaa"),
    );
    sighting_on(
        &rig,
        "cli-claude-00000002",
        "fail",
        "boom",
        30.0,
        Some("aaaa"),
    );
    assert!(!master_only(&rig, &fake));
    assert!(rig.headless_dispatch.shared_failure().is_none());

    sighting(&rig, "cli-claude-00000003", "fail", "boom", 0.0);
    assert!(
        !master_only(&rig, &fake),
        "the old failures do not corroborate a current one"
    );

    sighting_on(
        &rig,
        "cli-claude-00000004",
        "fail",
        "boom",
        0.0,
        Some("cccc"),
    );
    assert!(master_only(&rig, &fake), "both trees include current green");
    assert!(rig.headless_dispatch.shared_failure().is_some());
}

#[test]
fn it_survives_reloading_headless_dispatch_from_the_store() {
    let file = DbFile::new("shared");
    let first = rig_on(
        &["cli-claude-00000001", "cli-claude-00000002"],
        file.open(),
        Timestamp::from_millis(START),
    );
    let first_fake = cli(&first, 4);
    first.hold("cli-claude-00000001", "T.1");
    first.hold("cli-claude-00000002", "T.2");
    sighting(&first, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&first, "cli-claude-00000002", "fail", "boom", 60.0);
    assert_eq!(messages(&first, &first_fake).len(), 2);

    // The restarted daemon reads no sightings at all: the shared failure comes from the store.
    let restarted = rig_on(&[], file.open(), first.clock.now().plus_seconds(5.0));
    let restarted_fake = cli(&restarted, 4);
    restarted.hold("cli-claude-00000001", "T.1");
    restarted.hold("cli-claude-00000002", "T.2");
    assert!(
        messages(&restarted, &restarted_fake).is_empty(),
        "each worker had its message"
    );
    assert!(master_only(&restarted, &restarted_fake));
    assert!(restarted.headless_dispatch.shared_failure().is_some());
}

#[test]
fn it_lapses_after_two_hours() {
    let (rig, fake) = pool();
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    tick(&rig);
    assert!(rig.headless_dispatch.shared_failure().is_some());

    rig.clock.advance(LAPSE_S - 60.0);
    tick(&rig);
    assert!(rig.headless_dispatch.shared_failure().is_some());
    rig.clock.advance(60.0);
    messages(&rig, &fake);
    assert!(rig.headless_dispatch.shared_failure().is_none());
    assert!(!master_only(&rig, &fake));
}

#[test]
fn a_release_after_the_tip_result_was_unknown_starts_it() {
    let (rig, _) = pool();
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 60.0);
    lock(&rig.host.sightings)[0].tip_unknown = 1;
    tick(&rig);
    assert!(
        rig.headless_dispatch.shared_failure().is_none(),
        "still holds its ticket"
    );

    rig.finish("cli-claude-00000001");
    tick(&rig);
    let why = rig.headless_dispatch.shared_failure().expect("started");
    assert!(
        why.contains("result at the tip could not be found or run"),
        "{why}"
    );
}

#[test]
fn it_is_reported_once_when_the_hub_has_no_open_regression_for_the_check() {
    let (rig, _) = pool();
    // An open ticket may mention a check it does not cover.
    rig.hub.insert_ticket(super::tests::detail(
        "R.1",
        "open",
        None,
        &["regression"],
        &format!("Also ran {CHECK} while investigating another failure."),
    ));
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    tick(&rig);

    // Retry after a failed lookup so the hold still has a chance to be reported.
    rig.hub.set_unreachable(true);
    rig.headless_dispatch.report_shared(&rig.ctx, "jarvis");
    assert!(rig.hub.justiciar_reports().is_empty());
    rig.hub.set_unreachable(false);
    rig.hub.set_open_regressions(Vec::new());
    rig.headless_dispatch.report_shared(&rig.ctx, "jarvis");
    rig.headless_dispatch.report_shared(&rig.ctx, "jarvis");
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].subject, format!("shared:{CHECK}"));
    assert!(reports[0].on_machine);
}

#[test]
fn a_grouped_regression_suppresses_the_report_and_lifts_the_hold() {
    let (rig, fake) = pool();
    let primary = "rust:beta::tests";
    let mut regression = super::tests::detail(
        "R.2",
        "open",
        None,
        &["regression"],
        &format!("{primary} fails at the tip"),
    );
    regression.ticket.title = format!("Regression: {primary}");
    let ticket = regression.ticket.id.clone();
    rig.hub.insert_ticket(regression);
    rig.hub.set_open_regressions(
        [primary, CHECK]
            .into_iter()
            .map(|check| OpenRegression {
                check: check.parse().unwrap(),
                ticket: ticket.clone(),
            })
            .collect(),
    );
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    let given = messages(&rig, &fake);
    assert_eq!(given.len(), 2, "workers are still told: {given:?}");
    rig.headless_dispatch.report_shared(&rig.ctx, "jarvis");
    assert!(rig.hub.justiciar_reports().is_empty());
    // Landings pass a check that fails at the tip too, so ordinary tickets start.
    assert!(!master_only(&rig, &fake));
    assert!(rig.headless_dispatch.shared_failure().is_none());
}

fn open_regression(rig: &Rig) {
    rig.hub.set_open_regressions(vec![OpenRegression {
        check: CHECK.parse().unwrap(),
        ticket: super::tests::ticket_id("R.3"),
    }]);
}

#[test]
fn a_regression_opened_after_the_report_lifts_the_hold() {
    let (rig, fake) = pool();
    rig.hub.set_open_regressions(Vec::new());
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    tick(&rig);
    rig.headless_dispatch.report_shared(&rig.ctx, "jarvis");
    assert_eq!(rig.hub.justiciar_reports().len(), 1);
    assert!(master_only(&rig, &fake));

    open_regression(&rig);
    rig.headless_dispatch.report_shared(&rig.ctx, "jarvis");
    assert_eq!(rig.hub.justiciar_reports().len(), 1, "reported once");
    assert!(!master_only(&rig, &fake));
}

#[test]
fn a_regression_does_not_lift_a_hold_on_an_unknown_tip_result() {
    let (rig, fake) = pool();
    open_regression(&rig);
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 60.0);
    lock(&rig.host.sightings)[0].tip_unknown = 1;
    rig.finish("cli-claude-00000001");
    tick(&rig);
    rig.headless_dispatch.report_shared(&rig.ctx, "jarvis");
    // Such landings are refused, so starting more tickets into it wastes them.
    assert!(rig.headless_dispatch.shared_failure().is_some());
    assert!(master_only(&rig, &fake));
}

#[test]
fn the_handoff_of_a_ticket_holding_master_back_still_goes_out() {
    let (rig, fake) = pool();
    sighting(&rig, "cli-claude-00000001", "fail", "boom", 120.0);
    sighting(&rig, "cli-claude-00000002", "fail", "boom", 60.0);
    messages(&rig, &fake);
    reserve(&rig, "R.2", "reservation-3", &["regression"]);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "R.2");
}
