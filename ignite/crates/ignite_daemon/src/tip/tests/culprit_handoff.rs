//! A machine much slower than the fastest free machine with its system hands a culprit search
//! to that machine instead of running it, and a machine nearly as fast searches itself.

use ignite_core::machine_speed::Speed;
use ignite_core::protocol::hub::CulpritSearcher;

use super::*;

/// A tip `ffff000` where `a` fails, with two candidate landings that changed its input.
fn failing_world() -> (World, Context, Commit, TipCheck) {
    let world = World::new();
    let ctx = world.ctx("m-2");
    for sha in ["cccc000", "ffff000"] {
        world.work.add_commit(sha, &["src/a/lib.rs"]);
    }
    let tip = commit("ffff000");
    world.land(&tip);
    let failing = check("a", 'a');
    world.work.set_checks(&tip, vec![failing.clone()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world
        .work
        .set_outcome(&commit("cccc000"), &failing.check, Outcome::Pass);
    (world, ctx, tip, failing)
}

#[test]
fn a_slow_machine_reports_the_failure_and_leaves_the_search_to_a_faster_one() {
    let (world, ctx, tip, failing) = failing_world();
    let jarvis = CulpritSearcher {
        machine: "jarvis".to_owned(),
        speed: Speed::ONE,
        here: Speed::try_from(0.4).unwrap(),
    };
    world.hub.set_culprit_searcher(Some(jarvis.clone()));

    world.verifier().verify(&ctx, &tip).unwrap();

    // Only the tip's own run: no probe ran here.
    let runs: Vec<Commit> = world.work.runs().into_iter().map(|run| run.1).collect();
    assert_eq!(runs, vec![tip.clone()]);
    let handoffs = world.hub.handoffs();
    let [handoff] = handoffs.as_slice() else {
        panic!("expected one handed-off search, got {handoffs:?}");
    };
    assert_eq!(handoff.inputs, failing.inputs);
    let report = &handoff.report;
    assert_eq!(
        report.search,
        Some(CulpritSearch::NotRun { why: jarvis.why() })
    );
    assert_eq!(
        (&report.last_good, &report.first_bad),
        (&commit("aaaa000"), &tip)
    );
    assert!(jarvis.why().contains("jarvis"), "{}", jarvis.why());
}

#[test]
fn a_machine_the_hub_names_no_faster_searcher_for_searches_itself() {
    let (world, ctx, tip, _) = failing_world();

    world.verifier().verify(&ctx, &tip).unwrap();

    assert!(world.hub.handoffs().is_empty());
    let report = &world.hub.regressions()[0];
    assert_eq!(report.search, Some(CulpritSearch::Bisected { runs: 1 }));
    assert_eq!(report.first_bad, tip);
}
