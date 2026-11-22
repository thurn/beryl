//! Landings that lose the race for the tip: the next pass gates in the same tree, and judges
//! again only the families whose inputs the commits landed since touch.

use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::ids::TreeId;
use ignite_core::protocol::client::Exit;

use super::{World, world_with_checks};
use crate::land::env::GateTree;

/// A world with two check families, `fa` reading `a.txt` and `fb` reading `b.txt`, and a ticket
/// in `tree-01` that edits `a.txt`.
fn two_families() -> Arc<World> {
    let world = world_with_checks(vec![("fa:a", vec!["a.txt"]), ("fb:b", vec!["b.txt"])]);
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    Arc::new(world)
}

/// While the landing's first gate runs, another machine lands `changes`.
fn lands_during_first_gate(world: &Arc<World>, changes: &'static [(&'static str, &'static str)]) {
    let other = world.clone();
    world.queue_hook(move || other.git.land_other(changes));
}

fn judged(world: &World) -> Vec<(PathBuf, Vec<String>)> {
    world.gate.judged.lock().unwrap().clone()
}

#[test]
fn a_lost_race_whose_landed_commits_touch_no_gated_input_pushes_without_a_second_gate() {
    let world = two_families();
    lands_during_first_gate(&world, &[("z.txt", "unread")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("in pass 2 ("), "{}", reply.text);
    assert!(reply.text.contains("carried  2 families"), "{}", reply.text);
    assert_eq!(judged(&world).len(), 1, "{:?}", judged(&world));
    let pushes = world.git.pushes();
    assert_eq!(pushes.len(), 1);
    // The pushed commit is the rebase onto the commit that landed during the first gate.
    let history = world.git.repo.lock().unwrap().history.clone();
    assert_eq!(history.len(), 3);
    assert_eq!(history[2], pushes[0].1);
    assert_eq!(world.git.parent_of(&pushes[0].1), Some(history[1].clone()));
}

#[test]
fn a_lost_race_judges_again_only_the_families_the_landed_commits_touch() {
    let world = two_families();
    lands_during_first_gate(&world, &[("b.txt", "theirs")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("in pass 2 ("), "{}", reply.text);
    let judged = judged(&world);
    assert_eq!(judged.len(), 2, "{judged:?}");
    assert_eq!(judged[0].1, ["fa", "fb"]);
    assert_eq!(judged[1].1, ["fb"]);
    assert_eq!(world.gate.passes.lock().unwrap()[1], ["fb:b"]);
    // The carried family's verdict is still part of the landing's gate.
    assert!(
        reply.text.contains("2 checks: 1 from cache, 1 run"),
        "{}",
        reply.text
    );
    assert!(reply.text.contains("carried  1 family"), "{}", reply.text);
}

#[test]
fn a_landing_keeps_its_landing_tree_across_passes() {
    let world = two_families();
    world.trees.landing.lock().unwrap().push(GateTree {
        tree: TreeId::new("landing-2").unwrap(),
        path: PathBuf::from("/ignite-test-trees/landing-2"),
    });
    // Something landed before the landing started, so its first pass takes a landing tree.
    world.git.land_other(&[("c.txt", "before")]);
    lands_during_first_gate(&world, &[("b.txt", "theirs")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("in pass 2 ("), "{}", reply.text);
    let leased = world.trees.leased.lock().unwrap().clone();
    assert_eq!(leased.len(), 1, "{leased:?}");
    let path = PathBuf::from(format!("/ignite-test-trees/{}", leased[0]));
    let judged = judged(&world);
    assert_eq!(judged.len(), 2, "{judged:?}");
    assert!(judged.iter().all(|(tree, _)| *tree == path), "{judged:?}");
    assert_eq!(world.git.pushes()[0].0, path);
    assert!(world.trees.held.lock().unwrap().is_empty());
    assert_eq!(world.trees.landing.lock().unwrap().len(), 2);
}

#[test]
fn a_rebased_landing_gates_in_its_ticket_tree_when_that_builds_less() {
    let world = two_families();
    world.trees.landing.lock().unwrap().push(GateTree {
        tree: TreeId::new("landing-2").unwrap(),
        path: PathBuf::from("/ignite-test-trees/landing-2"),
    });
    world
        .trees
        .ticket_builds_less
        .store(true, std::sync::atomic::Ordering::SeqCst);
    // Something landed before the landing started, which alone would take a landing tree.
    world.git.land_other(&[("c.txt", "before")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(world.trees.leased.lock().unwrap().is_empty());
    let ticket = PathBuf::from("/ignite-test-trees/tree-01");
    let judged = judged(&world);
    assert!(judged.iter().all(|(tree, _)| *tree == ticket), "{judged:?}");
}

#[test]
fn a_landing_that_started_in_its_ticket_tree_stays_there_after_a_lost_race() {
    let world = two_families();
    lands_during_first_gate(&world, &[("b.txt", "theirs")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(world.trees.leased.lock().unwrap().is_empty());
    let ticket = PathBuf::from("/ignite-test-trees/tree-01");
    let judged = judged(&world);
    assert_eq!(judged.len(), 2, "{judged:?}");
    assert!(judged.iter().all(|(tree, _)| *tree == ticket), "{judged:?}");
}

#[test]
fn a_failed_gate_after_a_lost_race_frees_the_kept_landing_tree() {
    let world = two_families();
    world.git.land_other(&[("c.txt", "before")]);
    let other = world.clone();
    world.queue_hook(move || {
        other.git.land_other(&[("b.txt", "theirs")]);
        other.gate.failing.lock().unwrap().insert("fb:b".to_owned());
    });

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::CheckFailed, "{}", reply.text);
    assert!(world.trees.held.lock().unwrap().is_empty());
    assert_eq!(world.trees.landing.lock().unwrap().len(), 1);
    assert!(world.git.pushes().is_empty());
}
