//! The tip budget: `master` moves once the best-scored checks within `[tip] seconds` pass, and
//! the rest run afterwards without holding it, until one of them fails.

use std::collections::BTreeMap;

use ignite_core::budget::DEFAULT_TIP_RATE;
use ignite_core::gate::{GateCheck, History};
use ignite_core::score::{AreaRecord, Cost};

use super::*;
use crate::checks::Scoring;

/// A record of `failures` in 20 runs after changes to `src`, at 100 billion a run.
fn scored(text: &str, failures: u32) -> GateCheck {
    GateCheck {
        family_startup: ignite_core::score::Cost::Billions(0.0),
        check: name(&format!("unit:{text}")),
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

fn ran_with_master(world: &World) -> Vec<(Vec<String>, Commit)> {
    world
        .work
        .masters
        .lock()
        .unwrap()
        .iter()
        .map(|(checks, master)| {
            let mut names: Vec<String> = checks.iter().map(ToString::to_string).collect();
            names.sort();
            (names, master.clone())
        })
        .collect()
}

#[test]
fn master_moves_once_the_budgeted_checks_pass_and_a_later_failure_holds_it() {
    let world = World::new();
    let mut ctx = world.ctx("m-1");
    // 250 billion at the default rate: two checks of 100 billion fit, a third does not.
    ctx.config.tip.seconds = 250.0 / DEFAULT_TIP_RATE;
    let mut verifier = world.verifier();
    world.work.add_landing("tk.30", &["bbbb000"], &["src/c"]);
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![
            scored("a", 9),
            scored("b", 6),
            scored("c", 1),
            scored("d", 0),
        ],
    });
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = vec![
        check("a", 'a'),
        check("b", 'b'),
        check("c", 'c'),
        check("d", 'd'),
    ];
    world.work.set_checks(&tip, checks.clone());
    world.work.set_outcome(&tip, &name("unit:c"), Outcome::Fail);

    let outcome = verifier.verify(&ctx, &tip).unwrap();

    // The two likeliest checks ran with `master` still behind; once they passed it moved, and
    // the other two ran at the tip.
    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.master(), tip);
    let old = commit("aaaa000");
    assert_eq!(
        ran_with_master(&world),
        [
            (vec!["unit:a".to_owned(), "unit:b".to_owned()], old.clone()),
            (vec!["unit:c".to_owned(), "unit:d".to_owned()], tip.clone()),
        ]
    );
    // The failure past the budget filed one regression.
    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].check, name("unit:c"));
    // The round's record says how the budget split the checks.
    let round = world.hub.tip_rounds().pop().unwrap();
    let budget = round.budget.unwrap();
    assert_eq!((budget.within, budget.after, budget.forced), (2, 2, 0));
    assert!((budget.within_billions - 200.0).abs() < 1e-9);
    assert!((budget.after_billions - 200.0).abs() < 1e-9);
    assert!(budget.master_s.is_some());
    // Every check past the budget ran, so the round left none unrun.
    assert!(
        budget.unrun.is_empty() && budget.ended_by.is_none(),
        "{budget:?}"
    );

    // The next landing leaves the failing check's inputs alone: its cached failure is run
    // again before `master` moves, fails, and holds `master` at the last tip.
    let next = commit("cccc000");
    world.land(&next);
    world.work.set_checks(&next, checks);
    world
        .work
        .set_outcome(&next, &name("unit:c"), Outcome::Fail);
    let outcome = verifier.verify(&ctx, &next).unwrap();
    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![name("unit:c")]
        }
    );
    assert_eq!(world.master(), tip);
    assert_eq!(world.hub.regressions().len(), 1, "reported once");
}

/// A check with an open regression ticket runs before `master` moves whatever its score, so a
/// fix that has not landed keeps holding `master`.
#[test]
fn an_open_regression_runs_before_master_whatever_the_budget() {
    let world = World::new();
    let mut ctx = world.ctx("m-1");
    ctx.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
    let mut broken = scored("c", 0);
    broken.regression = Some(ticket("tk.31"));
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![scored("a", 9), broken],
    });
    world.work.add_landing("tk.32", &["bbbb000"], &["src/c"]);
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), check("c", 'c')]);
    world.work.set_outcome(&tip, &name("unit:c"), Outcome::Fail);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![name("unit:c")]
        }
    );
    assert_eq!(world.master(), commit("aaaa000"));
}

/// Once `master` has moved, a newer `staging` tip ends the checks past the budget at the next
/// batch, even while their family is under way. The tip loop waits for the round, so a round
/// that ran every check past the budget kept every newer tip from being verified for hours.
#[test]
fn a_newer_tip_ends_the_checks_past_the_budget_at_the_next_batch() {
    let world = World::new();
    let mut ctx = world.ctx("m-1");
    ctx.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![scored("a", 9), scored("b", 0)],
    });
    world.work.add_landing("tk.33", &["bbbb000"], &["src/b"]);
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), check("b", 'b')]);
    // A landing arrives once `master` has moved, while the family past the budget is being
    // built and listed, before its first batch is claimed.
    let git = world.git.clone();
    let moved = tip.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || {
            if git.tip("master").unwrap() == moved {
                git.set_tip("staging", commit("cccc000"));
            }
        }),
    );

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.master(), tip);
    // Only the check within the budget ran; the one past it is left to the newer tip.
    assert_eq!(
        ran_with_master(&world),
        [(vec!["unit:a".to_owned()], commit("aaaa000"))]
    );
    // The round's record names the check it left and the tip that ended it, which the hub
    // reads when a landing later finds that check failing.
    let budget = world.hub.tip_rounds().pop().unwrap().budget.unwrap();
    assert_eq!(budget.unrun, BTreeMap::from([("unit".to_owned(), 1)]));
    assert_eq!(budget.ended_by, Some(commit("cccc000")));
}
