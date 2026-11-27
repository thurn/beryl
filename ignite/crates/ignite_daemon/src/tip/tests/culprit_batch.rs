//! Batched culprit probing and hub-level open-regression deduplication.

use std::sync::Arc;

use ignite_core::config::Os;
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::OpenRegression;
use ignite_core::tipround::{CulpritSearch, DeferredPlatform, PlatformVerification, TipCoverage};
use ignite_core::traits::RevertOutcome;

use super::*;

fn in_same_family(label: &str, path: &str) -> TipCheck {
    let mut c = check(label, label.chars().next().unwrap());
    c.family = "shared".to_owned();
    c.check = name(&format!("shared:{label}"));
    c.key.check = c.check.clone();
    c.inputs = vec![path.to_owned()];
    c
}

/// Co-failing checks in the same family share one probe per commit across `newest`, `master`,
/// and bisection instead of checking out and rebuilding the tree once per check.
#[test]
fn co_failing_checks_share_one_probe_per_commit() {
    let world = World::new();
    let master = world.master();
    let old = commit("9999000");
    let c1 = commit("bbbb001");
    let c2 = commit("bbbb002");
    let tip = commit("bbbb003");
    let newer = commit("cccc000");
    for (idx, sha) in [&c1, &c2, &tip].iter().enumerate() {
        world.work.add_landing(
            &format!("tk.{idx}"),
            &[sha.as_str()],
            &["src/shared/lib.rs"],
        );
    }
    world.land(&tip);
    world.git.set_tip("staging", newer.clone());
    world.git.set_changed(&tip, &newer, &["src/shared/lib.rs"]);
    world.hub.set_tip_coverage(TipCoverage {
        platforms: [(
            Os::Macos,
            PlatformVerification {
                through: master.clone(),
                deferred: Some(DeferredPlatform {
                    since: commit("9999111"),
                    last_good: old,
                }),
            },
        )]
        .into(),
        ..TipCoverage::default()
    });
    let checks: Vec<TipCheck> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|label| {
            let mut item = in_same_family(label, "src/shared/lib.rs");
            item.key.platform = Some(Os::Macos);
            item
        })
        .collect();
    for item in &checks {
        world.work.set_outcome(&tip, &item.check, Outcome::Fail);
        world.work.set_outcome(&newer, &item.check, Outcome::Fail);
        world.work.set_outcome(&master, &item.check, Outcome::Pass);
        world.work.set_outcome(&c2, &item.check, Outcome::Fail);
        world.work.set_outcome(&c1, &item.check, Outcome::Pass);
    }
    world.work.set_checks(&tip, checks.clone());
    let ctx = world.ctx("m-1");

    world.verifier().verify(&ctx, &tip).unwrap();

    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 4);
    for report in &reports {
        assert_eq!(report.first_bad, c2);
        assert_eq!(report.last_good, c1);
        assert_eq!(report.caused_by, Some(ticket("tk.1")));
        assert_eq!(report.search, Some(CulpritSearch::Bisected { runs: 2 }));
    }
    // Count how many times each probe commit was checked out after the initial tip run.
    let probe_calls = |at: &Commit| {
        world
            .work
            .runs()
            .into_iter()
            .filter(|(_, commit, _, _)| commit == at)
            .count()
    };
    assert_eq!(
        probe_calls(&newer),
        1,
        "newest probed once for all 4 checks"
    );
    assert_eq!(
        probe_calls(&master),
        1,
        "master probed once for all 4 checks"
    );
    assert_eq!(probe_calls(&c2), 1, "c2 probed once for all 4 checks");
    assert_eq!(probe_calls(&c1), 1, "c1 probed once for all 4 checks");
}

/// Co-failing checks in the same family whose culprits differ still each find their own culprit
/// while sharing the probes at commits they both inspect.
#[test]
fn co_failing_checks_with_different_culprits_each_find_their_own() {
    let world = World::new();
    let c1 = commit("bbbb001");
    let c2 = commit("bbbb002");
    let c3 = commit("bbbb003");
    for (idx, sha) in [&c1, &c2, &c3].iter().enumerate() {
        world.work.add_landing(
            &format!("tk.{idx}"),
            &[sha.as_str()],
            &["src/shared/lib.rs"],
        );
    }
    world.land(&c3);
    let first = in_same_family("a", "src/shared/lib.rs");
    let second = in_same_family("b", "src/shared/lib.rs");
    // `first` broke at c3; `second` broke at c2.
    world.work.set_outcome(&c3, &first.check, Outcome::Fail);
    world.work.set_outcome(&c2, &first.check, Outcome::Pass);
    world.work.set_outcome(&c1, &first.check, Outcome::Pass);
    world.work.set_outcome(&c3, &second.check, Outcome::Fail);
    world.work.set_outcome(&c2, &second.check, Outcome::Fail);
    world.work.set_outcome(&c1, &second.check, Outcome::Pass);
    world
        .work
        .set_checks(&c3, vec![first.clone(), second.clone()]);
    let ctx = world.ctx("m-1");

    world.verifier().verify(&ctx, &c3).unwrap();

    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].check, first.check);
    assert_eq!(reports[0].first_bad, c3);
    assert_eq!(reports[0].caused_by, Some(ticket("tk.2")));
    assert_eq!(reports[1].check, second.check);
    assert_eq!(reports[1].first_bad, c2);
    assert_eq!(reports[1].caused_by, Some(ticket("tk.1")));
    let c2_probes = world
        .work
        .runs()
        .into_iter()
        .filter(|(_, at, _, _)| at == &c2)
        .count();
    assert_eq!(c2_probes, 1, "c2 probed once and reused for second check");
}

/// A check already open as a regression on the hub is not re-bisected on another machine or a
/// newer tip when it cannot revert, but still participates in a stall revert once eligible.
#[test]
fn open_hub_regression_skips_redundant_bisection_unless_revert_eligible() {
    let world = World::new();
    let master = world.master();
    let old = commit("9999000");
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .add_landing("tk.42", &[tip.as_str()], &["src/shared/lib.rs"]);
    let mut failing = in_same_family("a", "src/shared/lib.rs");
    failing.key.platform = Some(Os::Macos);
    world.hub.set_open_regressions(vec![OpenRegression {
        check: failing.check.clone(),
        ticket: ticket("tk.open"),
    }]);
    world.hub.set_tip_coverage(TipCoverage {
        platforms: [(
            Os::Macos,
            PlatformVerification {
                through: master.clone(),
                deferred: Some(DeferredPlatform {
                    since: commit("9999111"),
                    last_good: old.clone(),
                }),
            },
        )]
        .into(),
        ..TipCoverage::default()
    });
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world
        .work
        .set_outcome(&master, &failing.check, Outcome::Fail);
    world.work.set_checks(&tip, vec![failing.clone()]);
    let mut ctx = world.ctx("m-2");

    // 1. Not stalled: skipped completely before probing master or bisecting.
    world.verifier().verify(&ctx, &tip).unwrap();
    assert!(world.hub.regressions().is_empty());
    assert!(
        !world
            .work
            .runs()
            .iter()
            .any(|(_, at, _, _)| at == &master || at == &old),
        "must not probe master or gap when already open and not stalled"
    );

    // 2. Stalled, but check already fails at master: probes master once, then skips gap bisection.
    ctx.config.revert.stall_minutes = Some(120);
    world.clock.advance(121.0 * 60.0);
    world
        .work
        .add_landing("tk.old", &[master.as_str()], &["src/shared/lib.rs"]);
    let before_runs = world.work.runs().len();
    world
        .verifier()
        .pausing_with(Arc::new(|_| {}))
        .verify(&ctx, &tip)
        .unwrap();
    let after_runs: Vec<_> = world.work.runs().into_iter().skip(before_runs).collect();
    assert!(after_runs.iter().any(|(_, at, _, _)| at == &master));
    assert!(
        !after_runs.iter().any(|(_, at, _, _)| at == &old),
        "must not bisect across gap after master probe fails on an open regression"
    );
    assert!(world.hub.regressions().is_empty());

    // 3. Stalled and check passes at master: still reverts the culprit landing.
    world
        .work
        .set_outcome(&master, &failing.check, Outcome::Pass);
    world.git.script_revert(RevertOutcome::Clean {
        commit: commit("cccc000"),
    });
    world
        .verifier()
        .pausing_with(Arc::new(|_| {}))
        .verify(&ctx, &tip)
        .unwrap();
    assert_eq!(world.hub.reverted_landings().len(), 1);
    assert_eq!(
        world.hub.reverted_landings()[0].ticket,
        Some(ticket("tk.42"))
    );
}
