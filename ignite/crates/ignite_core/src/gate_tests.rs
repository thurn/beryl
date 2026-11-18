use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::score::BILLIONS_PER_CPU_SECOND;

const AREA: &str = "rules";

/// A budget of `billions` billion instructions: that many seconds at one billion a second.
fn gate(threshold: f64, billions: f64) -> Budget {
    Budget {
        threshold,
        seconds: billions,
        rate: 1.0,
    }
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
        family_startup: Cost::Billions(0.0),
        needs: Vec::new(),
        group: None,
        regression: None,
    }
}

/// A check that fails half the time after changes to [`AREA`].
fn likely(check_name: &str, cost: f64) -> GateCheck {
    check(check_name, 50, 80, 0.5, cost)
}

fn none() -> BTreeSet<String> {
    BTreeSet::new()
}

fn unbuilt(product: &str) -> BTreeSet<String> {
    BTreeSet::from([product.to_owned()])
}

fn plan_for(checks: &[GateCheck], unbuilt: &BTreeSet<String>, gate: &Budget) -> GatePlan {
    plan_landing(checks, unbuilt, None, gate)
}

fn plan_landing(
    checks: &[GateCheck],
    unbuilt: &BTreeSet<String>,
    landing: Option<&TicketId>,
    gate: &Budget,
) -> GatePlan {
    let areas = [AREA.to_owned()];
    let input = GateInput {
        areas: &areas,
        checks,
        unbuilt,
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
    let replay = check("rust:session::replay::all", 1, 310, 0.01, 5.0);

    let plan = plan_for(&[replay, aura], &none(), &gate(0.004, 400.0));

    assert_eq!(names(&plan.run), ["script:aura/enters-attached/you"]);
    assert!((plan.run[0].score.unwrap() - 0.125).abs() < 1e-12);
    // A chance of 0.0036 over 5 billion, with no build charged: still below the threshold.
    assert_eq!(left_names(&plan.tip), ["rust:session::replay::all"]);
    assert!(((plan.tip[0].score * 10_000.0).round() - 7.0).abs() < 1e-9);
    assert!(plan.past_budget.is_empty());
}

#[test]
fn the_budget_defers_scored_checks_but_never_the_always_set_or_edited_checks() {
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

    let plan = plan_for(&checks, &none(), &gate(0.0001, 400.0));

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
    assert_eq!(left_names(&plan.past_budget), ["script:c"]);
    assert!((plan.scored_cost - 300.0).abs() < 1e-9);
    assert!((plan.forced_cost - 1600.0).abs() < 1e-9);
}

/// A gate whose candidates cost three budgets runs the best-scored third.
#[test]
fn candidates_costing_three_budgets_run_the_best_third() {
    // Nine checks of 100 billion each; the earlier in the alphabet, the likelier to fail.
    let checks: Vec<GateCheck> = (0..9)
        .map(|i| check(&format!("script:c{i}"), 9 - i, 20, 0.01, 100.0))
        .collect();

    let plan = plan_for(&checks, &none(), &gate(0.0001, 300.0));

    assert_eq!(names(&plan.run), ["script:c0", "script:c1", "script:c2"]);
    assert_eq!(plan.past_budget.len(), 6);
    assert_eq!(plan.past_budget[0].check.as_str(), "script:c3");
    assert!((plan.scored_cost - 300.0).abs() < 1e-9);
    assert!((cost_of(&plan.past_budget) - 600.0).abs() < 1e-9);
}

#[test]
fn a_check_needing_a_product_the_tree_never_built_is_left_to_the_tip() {
    let mut cold = likely("rust:a", 1.0);
    cold.needs = vec!["rust-tests".to_owned()];
    let warm = likely("script:b", 1.0);

    let plan = plan_for(
        &[cold.clone(), warm],
        &unbuilt("rust-tests"),
        &gate(0.004, 400.0),
    );

    assert_eq!(names(&plan.run), ["script:b"]);
    assert_eq!(left_names(&plan.cold), ["rust:a"]);
    assert!((plan.scored_cost - 1.0).abs() < 1e-9);

    // Once the tree has built it, an incremental rebuild is simply done, and nothing is charged
    // for it.
    let plan = plan_for(&[cold], &none(), &gate(0.004, 400.0));
    assert_eq!(names(&plan.run), ["rust:a"]);
    assert!((plan.scored_cost - 1.0).abs() < 1e-9);
}

#[test]
fn a_product_an_always_check_needs_is_built_for_it_and_warm_for_the_rest() {
    let mut clippy = likely("clippy:workspace", 1.0);
    clippy.always = true;
    clippy.needs = vec!["rust-tests".to_owned()];
    let mut test = check("rust:b", 0, 0, 0.04, 1.0);
    test.needs = vec!["rust-tests".to_owned()];

    let plan = plan_for(
        &[test.clone(), clippy],
        &unbuilt("rust-tests"),
        &gate(0.004, 400.0),
    );
    assert_eq!(names(&plan.run), ["clippy:workspace", "rust:b"]);
    assert!((plan.forced_cost - 1.0).abs() < 1e-9);
    assert!((plan.scored_cost - 1.0).abs() < 1e-9);

    // An edited check that needs it is built too; without one, the product stays cold.
    let mut edited = check("rust:edited", 0, 10, 0.0, 1.0);
    edited.edited = true;
    edited.needs = vec!["rust-tests".to_owned()];
    let plan = plan_for(
        &[test.clone(), edited],
        &unbuilt("rust-tests"),
        &gate(0.004, 400.0),
    );
    assert_eq!(names(&plan.run), ["rust:edited", "rust:b"]);
    let plan = plan_for(&[test], &unbuilt("rust-tests"), &gate(0.004, 400.0));
    assert_eq!(left_names(&plan.cold), ["rust:b"]);
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
    let plan = plan_for(&checks, &none(), &gate(0.005, 400.0));
    assert!(plan.run.is_empty());
    assert_eq!(left_names(&plan.tip), ["shots:a", "shots:b"]);

    // Above the threshold, they are chosen together, or deferred together when the pair does
    // not fit within the budget.
    let plan = plan_for(&checks, &none(), &gate(0.001, 400.0));
    assert_eq!(names(&plan.run), ["shots:a", "shots:b"]);
    let plan = plan_for(&checks, &none(), &gate(0.001, 10.0));
    assert!(plan.run.is_empty());
    assert_eq!(left_names(&plan.past_budget), ["shots:a", "shots:b"]);
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
    let missing = unbuilt("rust-tests");
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

    let plan = plan_for(&checks, &none(), &gate(0.004, 400.0));

    assert_eq!(names(&plan.run), ["rust:old", "rust:new"]);
    assert!((plan.run[1].score.unwrap() - 0.25).abs() < 1e-12);
}

#[test]
fn a_cost_in_cpu_seconds_converts_for_the_budget() {
    let mut windows = likely("rust:measured-on-windows", 0.0);
    windows.history.as_mut().unwrap().median_cost = Cost::CpuSeconds(10.0);
    let plan = plan_for(&[windows.clone()], &none(), &gate(0.0001, 400.0));
    assert!((plan.scored_cost - 170.0).abs() < 1e-9);
    let plan = plan_for(&[windows], &none(), &gate(0.0001, 100.0));
    assert_eq!(left_names(&plan.past_budget), ["rust:measured-on-windows"]);
}

#[test]
fn a_cached_check_is_answered_from_the_cache_without_spending_the_budget() {
    let mut cached = likely("script:cached", 300.0);
    cached.cached = true;
    let checks = [cached, likely("script:fresh", 200.0)];

    let plan = plan_for(&checks, &none(), &gate(0.0001, 250.0));

    assert_eq!(names(&plan.cached), ["script:cached"]);
    assert_eq!(names(&plan.run), ["script:fresh"]);
    assert!((plan.scored_cost - 200.0).abs() < 1e-9);
}

#[test]
fn a_cached_result_is_used_after_the_budget_is_spent() {
    let mut cached = likely("script:cached", 900.0);
    cached.cached = true;
    let checks = [likely("script:a", 100.0), likely("script:b", 400.0), cached];

    let plan = plan_for(&checks, &none(), &gate(0.0001, 150.0));

    assert_eq!(names(&plan.run), ["script:a"]);
    assert_eq!(names(&plan.cached), ["script:cached"]);
    assert_eq!(left_names(&plan.past_budget), ["script:b"]);
}

#[test]
fn a_check_with_an_open_regression_is_left_out_unless_its_fix_is_landing() {
    let ticket = TicketId::new("Thrh4.90").unwrap();
    let mut broken = likely("script:broken", 1.0);
    broken.regression = Some(ticket.clone());
    let checks = [broken];

    let plan = plan_for(&checks, &none(), &gate(0.004, 400.0));
    assert!(plan.run.is_empty() && plan.tip.is_empty());
    assert_eq!(
        plan.known_failing,
        [(name("script:broken"), ticket.clone())]
    );

    let plan = plan_landing(&checks, &none(), Some(&ticket), &gate(0.004, 0.0));
    assert_eq!(names(&plan.run), ["script:broken"]);
    assert_eq!(plan.run[0].why, Why::Fixes);
    assert!(plan.known_failing.is_empty());
}

/// A family whose checks each cost 1 billion and whose command costs 10 to start.
fn started(check_name: &str) -> GateCheck {
    GateCheck {
        family_startup: Cost::Billions(10.0),
        ..likely(check_name, 1.0)
    }
}

/// b3d-interface checks recorded 0.7 CPU seconds each when run 50 to a command, most of it the
/// command's start-up, so a budget took a few of them for what all 50 cost. The start-up is now
/// charged once for the family, and each check costs what it adds.
#[test]
fn a_family_pays_its_start_up_once() {
    let checks: Vec<GateCheck> = (0..5).map(|i| started(&format!("tests:c{i}"))).collect();
    let plan = plan_for(&checks, &none(), &gate(0.0, 15.0));
    assert_eq!(plan.run.len(), 5, "{plan:?}");
    assert!(
        (plan.scored_cost - 15.0).abs() < 1e-9,
        "{}",
        plan.scored_cost
    );
    assert!(plan.run.iter().all(|p| (p.cost - 1.0).abs() < 1e-9));

    // The start-up alone does not fit, so no check of the family runs.
    let plan = plan_for(&checks, &none(), &gate(0.0, 10.5));
    assert!(plan.run.is_empty(), "{plan:?}");
    assert_eq!(plan.past_budget.len(), 5);
}

#[test]
fn a_family_started_by_a_check_that_must_run_charges_the_budget_nothing_more() {
    let mut checks: Vec<GateCheck> = (0..3).map(|i| started(&format!("tests:c{i}"))).collect();
    checks[0].always = true;
    let plan = plan_for(&checks, &none(), &gate(0.0, 2.0));
    assert_eq!(plan.run.len(), 3, "{plan:?}");
    assert!((plan.forced_cost - 11.0).abs() < 1e-9);
    assert!((plan.scored_cost - 2.0).abs() < 1e-9);
}

/// A check recorded as free whose family has not started yet cost its start-up, which used to
/// leave the selection looping once the budget was spent.
#[test]
fn a_free_check_that_would_start_a_family_is_left_once_the_budget_is_spent() {
    let free = GateCheck {
        family_startup: Cost::Billions(10.0),
        ..likely("other:free", 0.0)
    };
    let checks = vec![likely("tests:a", 5.0), free];
    // The free check scores best, and its start-up does not fit, which spends the budget.
    let plan = plan_for(&checks, &none(), &gate(0.0, 6.0));
    assert!(plan.run.is_empty(), "{plan:?}");
    assert_eq!(plan.past_budget.len(), 2);
}
