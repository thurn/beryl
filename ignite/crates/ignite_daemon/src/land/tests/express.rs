//! Express landings and the release branch moved over a hand-pushed commit.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use ignite_core::config::Express;
use ignite_core::ids::{Commit, TreeId};
use ignite_core::protocol::client::Exit;

use super::{World, world_with_checks};
use crate::land::Landing;
use crate::land::env::GateTree;
use crate::land::express::{ExpressLane, ExpressRun};
use crate::land::records::Records;

/// An express lane that records the commits it ran on and fails when told to.
#[derive(Default)]
struct FakeExpress {
    ran: Mutex<Vec<Commit>>,
    fails: bool,
}

impl ExpressLane for FakeExpress {
    fn tree(&self) -> GateTree {
        GateTree {
            tree: TreeId::new("express").unwrap(),
            path: PathBuf::from("/trees/express"),
        }
    }

    fn run(&self, commit: &Commit) -> ExpressRun {
        self.ran.lock().unwrap().push(commit.clone());
        ExpressRun {
            ran: vec![("cargo test".to_owned(), 12.0)],
            failure: self
                .fails
                .then(|| ("cargo test".to_owned(), "test x ... FAILED".to_owned())),
            cores: 3,
        }
    }

    fn run_in(&self, _tree: &std::path::Path) -> ExpressRun {
        unreachable!("a landing runs in the express tree")
    }
}

/// A world with a release branch, the express commands configured for `ignite/**`, and the
/// express lane `lane` on the machine.
fn express_world(lane: &Arc<FakeExpress>) -> World {
    let mut world = world_with_checks(vec![
        ("unit:a", vec!["a.txt"]),
        ("rel:smoke", vec!["ignite/lib.rs"]),
    ]);
    let config = &mut world.ctx.config;
    config.repository.release = Some("release".to_owned());
    config.repository.release_family = Some("rel".to_owned());
    config.express = Some(Express {
        paths: vec!["ignite/**".to_owned()],
        tree: PathBuf::from("trees/express"),
        dir: "ignite".to_owned(),
        commands: vec!["cargo test".to_owned()],
        families: Vec::new(),
    });
    let pause_clock = world.clock.clone();
    world.landing = Arc::new(Landing {
        trees: world.trees.clone(),
        gate: world.gate.clone(),
        records: Records::new(&world.records),
        express: Some(lane.clone()),
        pause: Arc::new(move |seconds| pause_clock.advance(seconds)),
    });
    world
}

fn released(world: &World) -> Vec<String> {
    let repo = world.git.repo.lock().unwrap();
    repo.branch_pushes
        .iter()
        .map(|(branch, _)| branch.clone())
        .chain(repo.replaces.iter().map(|(branch, ..)| branch.clone()))
        .collect()
}

#[test]
fn a_landing_that_changes_only_ignite_runs_the_express_commands_instead_of_the_gate() {
    let lane = Arc::new(FakeExpress::default());
    let world = express_world(&lane);
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .contains("express  1 commands passed in 12 s on 3 reserved cores"),
        "{}",
        reply.text
    );
    assert!(!reply.text.contains("gate     "), "{}", reply.text);
    let pushes = world.git.pushes();
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0].0, PathBuf::from("/trees/express"));
    assert_eq!(*lane.ran.lock().unwrap(), [pushes[0].1.clone()]);
    assert!(world.gate.passes.lock().unwrap().is_empty());
    // The express commands stand in for the release family, which is not run again.
    assert!(world.gate.family_runs.lock().unwrap().is_empty());
    assert_eq!(released(&world), ["release"]);
    assert_eq!(world.landings().len(), 1);
    // No landing tree was taken.
    assert_eq!(world.trees.landing.lock().unwrap().len(), 1);
}

#[test]
fn a_landing_that_also_changes_other_files_goes_through_the_gate() {
    let lane = Arc::new(FakeExpress::default());
    let world = express_world(&lane);
    world.hold(
        "tree-01",
        "t.1",
        "local_a",
        &[("ignite/lib.rs", "new"), ("a.txt", "mine")],
    );

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(lane.ran.lock().unwrap().is_empty());
    assert_eq!(world.gate.passes.lock().unwrap().len(), 1);
    assert!(!reply.text.contains("express  "), "{}", reply.text);
}

#[test]
fn a_failing_express_command_lands_nothing_and_shows_its_output() {
    let lane = Arc::new(FakeExpress {
        fails: true,
        ..FakeExpress::default()
    });
    let world = express_world(&lane);
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::CheckFailed, "{}", reply.text);
    assert!(reply.text.contains("`cargo test` failed"), "{}", reply.text);
    assert!(reply.text.contains("test x ... FAILED"), "{}", reply.text);
    assert!(
        reply.text.contains("rerun    cd ignite && cargo test"),
        "{}",
        reply.text
    );
    assert!(world.git.pushes().is_empty());
    assert!(world.landings().is_empty());
    assert_eq!(world.hub.holder("staging"), None);
    assert!(released(&world).is_empty());
}

/// The release branch holds a commit pushed there by hand, which the landing rebased: the
/// fast-forward push is refused, and the landing moves the branch over that commit because it
/// holds its changes.
#[test]
fn the_release_branch_is_moved_over_a_hand_pushed_copy_of_the_landed_change() {
    let lane = Arc::new(FakeExpress::default());
    let world = express_world(&lane);
    let by_hand = Commit::new("e".repeat(40)).unwrap();
    world
        .git
        .repo
        .lock()
        .unwrap()
        .other_tips
        .insert("release".to_owned(), by_hand.clone());
    world
        .git
        .other_branch_rejected
        .store(true, Ordering::SeqCst);
    world.git.copies_hold.store(true, Ordering::SeqCst);
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let landed = world.git.pushes()[0].1.clone();
    assert_eq!(
        world.git.repo.lock().unwrap().replaces,
        [("release".to_owned(), landed.clone(), by_hand)]
    );
    assert!(
        reply
            .text
            .contains(&format!("next     ignite wait deploy-{landed}")),
        "{}",
        reply.text
    );
}

#[test]
fn the_release_branch_is_not_moved_over_a_commit_whose_changes_the_landing_lacks() {
    let lane = Arc::new(FakeExpress::default());
    let world = express_world(&lane);
    let foreign = Commit::new("e".repeat(40)).unwrap();
    world
        .git
        .repo
        .lock()
        .unwrap()
        .other_tips
        .insert("release".to_owned(), foreign);
    world
        .git
        .other_branch_rejected
        .store(true, Ordering::SeqCst);
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .contains("release not moved: it holds eeeeeee, whose changes to ignite/"),
        "{}",
        reply.text
    );
    assert!(world.git.repo.lock().unwrap().replaces.is_empty());
}
