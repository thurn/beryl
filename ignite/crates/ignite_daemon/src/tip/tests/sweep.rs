//! A failed round runs the checks past the tip budget that its culprit may have broken, and
//! reports their failures in the same round.

use std::collections::BTreeMap;

use ignite_core::budget::DEFAULT_TIP_RATE;
use ignite_core::gate::{GateCheck, History};
use ignite_core::score::{AreaRecord, Cost};

use super::*;
use crate::checks::Scoring;

/// A record of `failures` in 20 runs after changes to `src`, at 100 billion a run.
fn scored(text: &str, failures: u32) -> GateCheck {
    GateCheck {
        family_startup: Cost::Billions(0.0),
        check: name(text),
        always: false,
        edited: false,
        cached: false,
        history: Some(History {
            areas: BTreeMap::from([("src".to_owned(), AreaRecord { runs: 20, failures })]),
            median_cost: Cost::Billions(100.0),
        }),
        family_rate: 0.01,
        family_median_cost: Cost::Billions(100.0),
        needs: Vec::new(),
        group: None,
        regression: None,
    }
}

/// A world whose tip `tip` lists `unit:a` to `unit:d` with a budget that fits only `unit:a`,
/// which fails there, as `failing` do. Returns the context to verify with.
fn budgeted(world: &World, tip: &Commit, failing: &[&str]) -> Context {
    let mut ctx = world.ctx("m-1");
    ctx.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![
            scored("unit:a", 9),
            scored("unit:b", 1),
            scored("unit:c", 1),
            scored("unit:d", 0),
        ],
    });
    world.land(tip);
    world.work.set_checks(
        tip,
        vec![
            check("a", 'a'),
            check("b", 'b'),
            check("c", 'c'),
            check("d", 'd'),
        ],
    );
    for text in failing {
        world.work.set_outcome(tip, &name(text), Outcome::Fail);
    }
    ctx
}

/// The checks run at `at`, in order.
fn ran_at(world: &World, at: &Commit) -> Vec<String> {
    world
        .work
        .runs()
        .into_iter()
        .filter(|(_, commit, _, _)| commit == at)
        .flat_map(|(_, _, checks, _)| checks)
        .map(|check| check.to_string())
        .collect()
}

#[test]
fn a_culprit_that_breaks_checks_past_the_budget_has_them_all_reported_in_one_round() {
    let world = World::new();
    let tip = commit("bbbb000");
    world
        .work
        .add_landing("tk.30", &["bbbb000"], &["src/a", "src/b", "src/c"]);
    let ctx = budgeted(&world, &tip, &["unit:a", "unit:b", "unit:c"]);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![name("unit:a")],
            promoted: None,
        }
    );
    // `unit:d` reads nothing the culprit changed.
    assert_eq!(ran_at(&world, &tip), ["unit:a", "unit:b", "unit:c"]);
    let mut reported: Vec<(String, Option<TicketId>)> = world
        .hub
        .regressions()
        .into_iter()
        .map(|report| (report.check.to_string(), report.caused_by))
        .collect();
    reported.sort();
    let culprit = Some(ticket("tk.30"));
    assert_eq!(
        reported,
        [
            ("unit:a".to_owned(), culprit.clone()),
            ("unit:b".to_owned(), culprit.clone()),
            ("unit:c".to_owned(), culprit),
        ]
    );
    let budget = world.hub.tip_rounds().pop().unwrap().budget.unwrap();
    assert_eq!(budget.swept, 2);
    assert_eq!(budget.unrun, BTreeMap::from([("unit".to_owned(), 1)]));
}

#[test]
fn a_check_past_the_budget_whose_inputs_the_culprit_left_alone_waits() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.work.add_landing("tk.30", &["bbbb000"], &["src/a"]);
    // `unit:d` would fail too, but its inputs are not the culprit's.
    let ctx = budgeted(&world, &tip, &["unit:a", "unit:d"]);

    world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(ran_at(&world, &tip), ["unit:a"]);
    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].caused_by, Some(ticket("tk.30")));
    let budget = world.hub.tip_rounds().pop().unwrap().budget.unwrap();
    assert_eq!(budget.swept, 0);
}

#[test]
fn a_failure_with_no_single_culprit_runs_no_sweep() {
    let world = World::new();
    let tip = commit("dddd000");
    for (ticket_id, at) in [
        ("tk.1", "bbbb000"),
        ("tk.2", "cccc000"),
        ("tk.3", "dddd000"),
    ] {
        world
            .work
            .add_landing(ticket_id, &[at], &["src/a", "src/b", "src/c"]);
    }
    // The search cannot narrow the three candidates, so it ends with a range.
    world
        .work
        .set_outcome(&commit("bbbb000"), &name("unit:a"), Outcome::Incomplete);
    let ctx = budgeted(&world, &tip, &["unit:a", "unit:b", "unit:c"]);

    world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(ran_at(&world, &tip), ["unit:a"]);
    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].caused_by, None);
    assert_eq!(reports[0].suspects.len(), 3);
}
