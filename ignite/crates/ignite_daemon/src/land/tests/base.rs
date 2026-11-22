//! A gate that fails on a check which already fails at the tip the landing was rebased onto:
//! the landing compares the check there, from the cache or by running it, and lands with the
//! check known to fail only when it fails there too.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::protocol::Outcome;
use ignite_core::protocol::client::Exit;

use super::fakes::{key, store};
use super::{World, world_with_checks};

/// A world whose `unit:both` reads `a.txt` and `b.txt` and fails where `file` holds `text`.
fn broken(file: &'static str, text: &'static str) -> Arc<World> {
    let world = world_with_checks(vec![
        ("unit:both", vec!["a.txt", "b.txt"]),
        ("unit:c", vec!["c.txt"]),
    ]);
    world
        .gate
        .broken_by
        .lock()
        .unwrap()
        .push(("unit:both", file, text));
    Arc::new(world)
}

const LANDING_TREE: &str = "/ignite-test-trees/landing-1";

/// Caches `outcome` for `unit:both` at the tip, as a run there would have.
fn cache_at_tip(world: &World, outcome: Outcome) {
    let path = Path::new(LANDING_TREE);
    world.git.check_out(path, &world.git.tip_now());
    let key = key(&world.ctx, path, "unit:both", &["a.txt", "b.txt"]);
    store(&world.ctx, &key, outcome);
}

fn checks_runs(world: &World) -> Vec<(PathBuf, Vec<String>)> {
    world.gate.checks_runs.lock().unwrap().clone()
}

/// Another landing broke `unit:both` through `b.txt`, and this ticket edits `a.txt`, so the
/// check's input hash differs between the tip and the landing and the tip has no result.
#[test]
fn a_check_that_fails_at_the_tip_too_is_known_failing_and_its_regression_is_filed() {
    let world = broken("b.txt", "broken");
    world.git.land_other(&[("b.txt", "broken")]);
    let tip = world.git.tip_now();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains(
            "known    1 check left out as known-failing: they fail at the tip too: unit:both (tk.1)"
        ),
        "{}",
        reply.text
    );
    let regressions = world.hub.hub.regressions();
    assert_eq!(regressions.len(), 1, "{regressions:?}");
    assert_eq!(regressions[0].check.as_str(), "unit:both");
    assert_eq!(regressions[0].first_bad, tip);
    // The hub records why tip verification missed a check that a landing found failing.
    assert_eq!(
        regressions[0].found_by,
        ignite_core::protocol::hub::FoundBy::Landing
    );
    assert_eq!(
        checks_runs(&world),
        [(PathBuf::from(LANDING_TREE), vec!["unit:both".to_owned()])]
    );
    assert_eq!(world.git.pushes().len(), 1);
}

#[test]
fn a_failure_cached_at_the_tip_makes_the_check_known_failing_without_running_it_there() {
    let world = broken("b.txt", "broken");
    world.git.land_other(&[("b.txt", "broken")]);
    cache_at_tip(&world, Outcome::Fail);
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("unit:both (tk.1)"), "{}", reply.text);
    assert_eq!(world.hub.hub.regressions().len(), 1);
    assert_eq!(
        checks_runs(&world),
        [(PathBuf::from(LANDING_TREE), Vec::new())]
    );
}

#[test]
fn a_check_that_passes_at_the_tip_still_refuses_the_landing() {
    let world = broken("a.txt", "mine");
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::CheckFailed, "{}", reply.text);
    assert!(reply.text.contains("unit:both passes at"), "{}", reply.text);
    assert!(
        reply.text.contains("so the failure is this change's"),
        "{}",
        reply.text
    );
    assert!(world.hub.hub.regressions().is_empty());
    assert_eq!(
        checks_runs(&world),
        [(PathBuf::from(LANDING_TREE), vec!["unit:both".to_owned()])]
    );
    assert!(world.git.pushes().is_empty());
}

#[test]
fn a_pass_cached_at_the_tip_refuses_the_landing_without_running_the_check_there() {
    let world = broken("a.txt", "mine");
    cache_at_tip(&world, Outcome::Pass);
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::CheckFailed, "{}", reply.text);
    assert!(reply.text.contains("unit:both passes at"), "{}", reply.text);
    assert!(world.hub.hub.regressions().is_empty());
    assert_eq!(
        checks_runs(&world),
        [(PathBuf::from(LANDING_TREE), Vec::new())]
    );
}

/// The landing gates in the machine's only landing tree, so it checks the tip out there for the
/// comparison and the rebased commit again before the gate runs once more.
#[test]
fn with_no_free_landing_tree_the_landing_compares_in_its_own_and_moves_it_back() {
    let world = broken("b.txt", "broken");
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.land_other(&[("b.txt", "broken")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("unit:both (tk.1)"), "{}", reply.text);
    assert_eq!(
        checks_runs(&world),
        [(PathBuf::from(LANDING_TREE), vec!["unit:both".to_owned()])]
    );
    let pushes = world.git.pushes();
    assert_eq!(pushes.len(), 1);
    let checked_out = world.git.repo.lock().unwrap().checked_out[Path::new(LANDING_TREE)].clone();
    assert_eq!(checked_out, pushes[0].1);
}
