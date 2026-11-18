use std::collections::BTreeMap;

use super::*;
use crate::score::BILLIONS_PER_CPU_SECOND;

const AREA: &str = "rules";

fn gate(threshold: f64, cap: f64) -> Gate {
    Gate { threshold, cap }
}

fn name(check: &str) -> CheckName {
    CheckName::new(check).unwrap()
}

/// A check with `failures` in `runs` after changes to [`AREA`], costing `cost` billions.
fn check(check: &str, failures: u32, runs: u32, family_rate: f64, cost: f64) -> GateCheck {
    GateCheck {
        check: name(check),
        always: false,
        edited: false,
        cached: false,
        history: Some(History {
            areas: BTreeMap::from([(AREA.to_owned(), AreaRecord { runs, failures })]),
            median_cost: Cost::Billions(cost),
        }),
        family_rate,
        family_median_cost: Cost::Billions(1.0),
        needs: Vec::new(),
        group: None,
        regression: None,
    }
}

/// A check that fails half the time after changes to [`AREA`].
fn likely(check_name: &str, cost: f64) -> GateCheck {
    check(check_name, 50, 80, 0.5, cost)
}

fn plan_for(checks: &[GateCheck], missing: &BTreeMap<String, f64>, gate: &Gate) -> GatePlan {
    plan_landing(checks, missing, None, gate)
}

fn plan_landing(
    checks: &[GateCheck],
    missing: &BTreeMap<String, f64>,
    landing: Option<&TicketId>,
    gate: &Gate,
) -> GatePlan {
    let areas = [AREA.to_owned()];
    let input = GateInput {
        areas: &areas,
        checks,
        missing_builds: missing,
        landing,
        billions_per_cpu_second: BILLIONS_PER_CPU_SECOND,
    };
    plan(&input, gate)
}

fn names(planned: &[Planned]) -> Vec<&str> {
    planned
        .iter()
        .map(|planned| planned.check.as_str())
        .collect()
}

fn left_names(left: &[Left]) -> Vec<&str> {
    left.iter().map(|left| left.check.as_str()).collect()
}

/// The two checks of the worked example under "Scores" in `docs/ignite.md`.
#[test]
fn worked_example() {
    let aura = check("script:aura/enters-attached/you", 9, 74, 0.02, 0.8);
    let mut replay = check("rust:session::replay::all", 1, 310, 0.01, 5.0);
    replay.needs = vec!["rust-tests".to_owned()];
    let missing = BTreeMap::from([("rust-tests".to_owned(), 90.0)]);

    let plan = plan_for(&[replay, aura], &missing, &gate(0.004, 400.0));

    assert_eq!(names(&plan.run), ["script:aura/enters-attached/you"]);
    assert!((plan.run[0].score.unwrap() - 0.125).abs() < 1e-12);
    assert_eq!(left_names(&plan.tip), ["rust:session::replay::all"]);
    assert!(((plan.tip[0].score * 100_000.0).round() - 4.0).abs() < 1e-9);
    assert!(plan.past_cap.is_empty());
}

#[test]
fn the_cap_defers_scored_checks_but_never_the_always_set_or_edited_checks() {
    let mut clippy = likely("clippy:workspace", 1000.0);
    clippy.always = true;
    let mut edited = check("rust:rules::tests::new", 0, 500, 0.0, 600.0);
    edited.edited = true;
    let checks = [
        clippy,
        edited,
        likely("script:a", 150.0),
        likely("script:b", 150.0),
        likely("script:c", 150.0),
    ];

    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.0001, 400.0));

    assert_eq!(
        names(&plan.run),
        [
            "clippy:workspace",
            "rust:rules::tests::new",
            "script:a",
            "script:b"
        ]
    );
    assert_eq!(plan.run[0].why, Why::Always);
    assert_eq!(plan.run[1].why, Why::Edited);
    assert_eq!(left_names(&plan.past_cap), ["script:c"]);
    assert!((plan.scored_cost - 300.0).abs() < 1e-9);
    assert!((plan.forced_cost - 1600.0).abs() < 1e-9);
}

#[test]
fn a_shared_build_is_charged_once_and_the_rest_are_scored_again_without_it() {
    let mut first = likely("rust:a", 1.0);
    first.needs = vec!["rust-tests".to_owned()];
    // With the build its score is 0.04 / 101, below the threshold; without it, 0.04.
    let mut second = check("rust:b", 0, 0, 0.04, 1.0);
    second.needs = vec!["rust-tests".to_owned()];
    let missing = BTreeMap::from([("rust-tests".to_owned(), 100.0)]);

    let plan = plan_for(&[second, first], &missing, &gate(0.004, 400.0));

    assert_eq!(names(&plan.run), ["rust:a", "rust:b"]);
    assert_eq!(plan.run[0].builds, ["rust-tests"]);
    assert!(plan.run[1].builds.is_empty());
    assert!((plan.scored_cost - 102.0).abs() < 1e-9);
}

#[test]
fn a_build_an_always_check_needs_is_paid_before_scoring() {
    let mut clippy = likely("clippy:workspace", 1.0);
    clippy.always = true;
    clippy.needs = vec!["rust-tests".to_owned()];
    let mut test = check("rust:b", 0, 0, 0.04, 1.0);
    test.needs = vec!["rust-tests".to_owned()];
    let missing = BTreeMap::from([("rust-tests".to_owned(), 100.0)]);

    let plan = plan_for(&[test, clippy], &missing, &gate(0.004, 400.0));

    assert_eq!(names(&plan.run), ["clippy:workspace", "rust:b"]);
    assert_eq!(plan.run[0].builds, ["rust-tests"]);
    assert!((plan.forced_cost - 101.0).abs() < 1e-9);
    assert!((plan.scored_cost - 1.0).abs() < 1e-9);
}

#[test]
fn grouped_checks_are_scored_on_their_summed_cost_and_kept_together() {
    let grouped = |check_name: &str| {
        let mut check = check(check_name, 0, 0, 0.04, 6.0);
        check.group = Some("g1".to_owned());
        check
    };
    // Alone each scores 0.04 / 6, above the threshold; together 0.04 / 12, below it.
    let checks = [grouped("shots:a"), grouped("shots:b")];
    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.005, 400.0));
    assert!(plan.run.is_empty());
    assert_eq!(left_names(&plan.tip), ["shots:a", "shots:b"]);

    // Above the threshold, they are chosen together, or deferred together when the pair does
    // not fit under the cap.
    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.001, 400.0));
    assert_eq!(names(&plan.run), ["shots:a", "shots:b"]);
    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.001, 10.0));
    assert!(plan.run.is_empty());
    assert_eq!(left_names(&plan.past_cap), ["shots:a", "shots:b"]);
}

#[test]
fn the_same_inputs_give_the_same_plan_in_any_order() {
    let mut checks = vec![
        likely("script:a", 2.0),
        likely("script:b", 2.0),
        check("script:c", 1, 40, 0.01, 3.0),
        check("rust:d", 0, 100, 0.001, 50.0),
        likely("script:e", 200.0),
    ];
    checks[3].needs = vec!["rust-tests".to_owned()];
    checks[4].group = Some("g".to_owned());
    let missing = BTreeMap::from([("rust-tests".to_owned(), 30.0)]);
    let gate = gate(0.001, 100.0);

    let forward = plan_for(&checks, &missing, &gate);
    checks.reverse();
    assert_eq!(plan_for(&checks, &missing, &gate), forward);
    checks.swap(0, 3);
    assert_eq!(plan_for(&checks, &missing, &gate), forward);
    // Equal scores are chosen in name order.
    assert_eq!(names(&forward.run)[..2], ["script:a", "script:b"]);
}

#[test]
fn a_check_never_run_is_certain_to_fail_at_its_family_median_cost() {
    let mut new = check("rust:new", 0, 0, 0.0, 0.0);
    new.history = None;
    new.family_median_cost = Cost::Billions(4.0);
    let checks = [likely("rust:old", 1.0), new];

    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.004, 400.0));

    assert_eq!(names(&plan.run), ["rust:old", "rust:new"]);
    assert!((plan.run[1].score.unwrap() - 0.25).abs() < 1e-12);
}

#[test]
fn a_cost_in_cpu_seconds_converts_for_the_cap() {
    let mut windows = likely("rust:measured-on-windows", 0.0);
    windows.history.as_mut().unwrap().median_cost = Cost::CpuSeconds(10.0);
    let plan = plan_for(&[windows.clone()], &BTreeMap::new(), &gate(0.0001, 400.0));
    assert!((plan.scored_cost - 170.0).abs() < 1e-9);
    let plan = plan_for(&[windows], &BTreeMap::new(), &gate(0.0001, 100.0));
    assert_eq!(left_names(&plan.past_cap), ["rust:measured-on-windows"]);
}

#[test]
fn a_cached_check_is_answered_from_the_cache_without_spending_the_cap() {
    let mut cached = likely("script:cached", 300.0);
    cached.cached = true;
    let checks = [cached, likely("script:fresh", 200.0)];

    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.0001, 250.0));

    assert_eq!(names(&plan.cached), ["script:cached"]);
    assert_eq!(names(&plan.run), ["script:fresh"]);
    assert!((plan.scored_cost - 200.0).abs() < 1e-9);
}

#[test]
fn a_cached_result_is_used_after_the_cap_is_reached() {
    let mut cached = likely("script:cached", 900.0);
    cached.cached = true;
    let checks = [likely("script:a", 100.0), likely("script:b", 400.0), cached];

    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.0001, 150.0));

    assert_eq!(names(&plan.run), ["script:a"]);
    assert_eq!(names(&plan.cached), ["script:cached"]);
    assert_eq!(left_names(&plan.past_cap), ["script:b"]);
}

#[test]
fn a_check_with_an_open_regression_is_left_out_unless_its_fix_is_landing() {
    let ticket = TicketId::new("Thrh4.90").unwrap();
    let mut broken = likely("script:broken", 1.0);
    broken.regression = Some(ticket.clone());
    let checks = [broken];

    let plan = plan_for(&checks, &BTreeMap::new(), &gate(0.004, 400.0));
    assert!(plan.run.is_empty() && plan.tip.is_empty());
    assert_eq!(
        plan.known_failing,
        [(name("script:broken"), ticket.clone())]
    );

    let plan = plan_landing(&checks, &BTreeMap::new(), Some(&ticket), &gate(0.004, 0.0));
    assert_eq!(names(&plan.run), ["script:broken"]);
    assert_eq!(plan.run[0].why, Why::Fixes);
    assert!(plan.known_failing.is_empty());
}
