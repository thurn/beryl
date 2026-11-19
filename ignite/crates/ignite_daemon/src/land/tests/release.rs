//! When a landing judges the release family, and what it does after its push before replying.

use std::sync::{Arc, Mutex};

use ignite_core::protocol::client::Exit;

use super::fakes::CacheGate;
use super::{World, world_with};
use crate::land::Landing;
use crate::land::records::Records;

/// A world with a release branch whose gate runs `checks`, leaves the checks in `left` to tip
/// verification and does not list the families in `cold`.
fn release_world(
    checks: Vec<(&'static str, Vec<&'static str>)>,
    left: Vec<&'static str>,
    cold: Vec<&'static str>,
) -> World {
    let mut world = world_with(checks.clone(), Vec::new());
    let repository = &mut world.ctx.config.repository;
    repository.release = Some("release".to_owned());
    repository.release_family = Some("rel".to_owned());
    world.gate = Arc::new(CacheGate {
        checks,
        left,
        cold,
        ..CacheGate::default()
    });
    let pause_clock = world.clock.clone();
    world.landing = Arc::new(Landing {
        trees: world.trees.clone(),
        gate: world.gate.clone(),
        records: Records::new(&world.records),
        express: None,
        pause: Arc::new(move |seconds| pause_clock.advance(seconds)),
    });
    world
}

fn released(world: &World) -> Vec<String> {
    let repo = world.git.repo.lock().unwrap();
    repo.branch_pushes
        .iter()
        .map(|(branch, _)| branch.clone())
        .collect()
}

/// A gate that left only checks of other families to tip verification ran the whole release
/// family, so its outcomes move the release without running the family again.
#[test]
fn checks_of_other_families_left_to_tip_do_not_rerun_the_release_family() {
    let world = release_world(
        vec![
            ("unit:a", vec!["a.txt"]),
            ("unit:slow", vec!["ignite/lib.rs"]),
            ("rel:smoke", vec!["ignite/lib.rs"]),
        ],
        vec!["unit:slow"],
        Vec::new(),
    );
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .contains("tip      1 checks left to tip verification"),
        "{}",
        reply.text
    );
    assert!(world.gate.family_runs.lock().unwrap().is_empty());
    assert_eq!(released(&world), ["release"]);
}

/// A release family check the gate left to tip verification has the family run before the
/// push.
#[test]
fn a_release_check_left_to_tip_has_the_family_run_before_the_push() {
    let world = release_world(
        vec![
            ("rel:smoke", vec!["ignite/lib.rs"]),
            ("rel:slow", vec!["ignite/lib.rs"]),
        ],
        vec!["rel:slow"],
        Vec::new(),
    );
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(*world.gate.family_runs.lock().unwrap(), ["rel"]);
    assert_eq!(released(&world), ["release"]);
    // The family's run took its checks back from tip verification.
    assert!(!reply.text.contains("tip      "), "{}", reply.text);
}

/// A release family the gate did not list, because listing it needs a product the tree has
/// never built, is built and run before the push, and its run counts with the gate.
#[test]
fn an_unlisted_release_family_is_run_before_the_push() {
    let world = release_world(
        vec![
            ("unit:a", vec!["a.txt"]),
            ("rel:smoke", vec!["ignite/lib.rs"]),
        ],
        Vec::new(),
        vec!["rel"],
    );
    let pushes_at_family_run = Arc::new(Mutex::new(None));
    let slot = pushes_at_family_run.clone();
    let git = world.git.clone();
    *world.gate.on_family.lock().unwrap() = Some(Box::new(move || {
        *slot.lock().unwrap() = Some(git.pushes().len());
    }));
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(*world.gate.family_runs.lock().unwrap(), ["rel"]);
    assert_eq!(*pushes_at_family_run.lock().unwrap(), Some(0));
    assert_eq!(released(&world), ["release"]);
    assert!(
        !reply
            .text
            .contains("not listed, as listing needs a product"),
        "{}",
        reply.text
    );
    assert!(reply.text.contains("gate     2 checks"), "{}", reply.text);
}

/// Once the landing has pushed, the ticket closes and its tree is released before the release
/// branch is pushed, which is the only work left before the reply.
#[test]
fn the_ticket_tree_is_released_before_the_release_branch_is_pushed() {
    let world = release_world(
        vec![("rel:smoke", vec!["ignite/lib.rs"])],
        Vec::new(),
        Vec::new(),
    );
    let at_release_push = Arc::new(Mutex::new(None));
    let slot = at_release_push.clone();
    let trees = world.trees.clone();
    let hub = world.hub.clone();
    let gate = world.gate.clone();
    *world.git.on_branch_push.lock().unwrap() = Some(Box::new(move || {
        *slot.lock().unwrap() = Some((
            trees.released.lock().unwrap().len(),
            hub.hub.landings().len(),
            gate.passes.lock().unwrap().len(),
        ));
    }));
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(*at_release_push.lock().unwrap(), Some((1, 1, 1)));
    assert!(world.gate.family_runs.lock().unwrap().is_empty());
    assert!(
        reply.text.contains("release  release moved to"),
        "{}",
        reply.text
    );
}
