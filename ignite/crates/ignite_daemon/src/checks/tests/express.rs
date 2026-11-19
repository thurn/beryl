//! `ignite test` and `ignite check` on the express lane: a tree that changes only the express
//! paths runs the express commands in place of the express families' checks.

use super::*;
use crate::checks::verbs::{CheckVerb, TestVerb};
use crate::land::env::GateTree;
use crate::land::express::{ExpressLane, ExpressRun};

/// An express lane that records the trees it ran in and fails when told to.
#[derive(Default)]
struct FakeLane {
    ran_in: Mutex<Vec<PathBuf>>,
    fails: bool,
}

impl ExpressLane for FakeLane {
    fn tree(&self) -> GateTree {
        unreachable!("a verb runs in the agent's tree")
    }

    fn run(&self, _commit: &Commit) -> ExpressRun {
        unreachable!("a verb runs in the agent's tree")
    }

    fn run_in(&self, tree: &Path) -> ExpressRun {
        self.ran_in.lock().unwrap().push(tree.to_path_buf());
        ExpressRun {
            ran: vec![("cargo test".to_owned(), 12.0)],
            failure: self
                .fails
                .then(|| ("cargo test".to_owned(), "test x ... FAILED".to_owned())),
            cores: 3,
            carried: false,
            reused_after_s: None,
        }
    }
}

/// The world of [`MANIFEST`] with a tree that changes `changed`, and the express commands
/// configured for `ignite/**` in place of the `unit` family.
fn express_world(changed: &[&str]) -> World {
    let mut world = world(MANIFEST, changed, FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:a\"}}\n{{\"check\": \"unit:b\"}}\n{NEXTEST_LISTING}\n");
    world.ctx.config.express = Some(ignite_core::config::Express {
        paths: vec!["ignite/**".to_owned()],
        tree: "express".into(),
        dir: "ignite".to_owned(),
        commands: vec!["cargo test".to_owned()],
        families: vec!["unit".to_owned()],
    });
    world
}

fn request(world: &World, verb: &str, args: &[&str]) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: world.dir.join("sub"),
        session: None,
        wait_s: 1,
        pid: None,
    }
}

fn test_verb(world: &World, lane: &Arc<FakeLane>) -> TestVerb {
    TestVerb {
        checks: world.checks.clone(),
        express: Some(lane.clone()),
    }
}

#[test]
fn testing_only_express_families_in_an_express_tree_runs_the_express_commands() {
    let world = express_world(&["ignite/src/lib.rs"]);
    let lane = Arc::new(FakeLane::default());
    let reply = test_verb(&world, &lane).run(&world.ctx, &request(&world, "test", &["unit:a"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        reply.text,
        "express  1 commands passed in 12 s on 3 cores, in place of the checks of \
         unit: cargo test\n"
    );
    assert_eq!(lane.ran_in.lock().unwrap().len(), 1);
    assert!(
        world.launcher.commands("runner").is_empty(),
        "nothing asked the scheduler"
    );
}

#[test]
fn a_failing_express_command_fails_the_verb_with_its_output() {
    let world = express_world(&["ignite/src/lib.rs"]);
    let lane = Arc::new(FakeLane {
        fails: true,
        ..FakeLane::default()
    });
    let reply = test_verb(&world, &lane).run(&world.ctx, &request(&world, "test", &["unit:b"]));
    assert_eq!(reply.exit, Exit::CheckFailed);
    assert!(reply.text.contains("`cargo test` failed"), "{}", reply.text);
    assert!(reply.text.contains("test x ... FAILED"), "{}", reply.text);
}

#[test]
fn a_prefix_outside_the_express_families_or_a_change_outside_the_paths_uses_the_scheduler() {
    let lane = Arc::new(FakeLane::default());
    let world = express_world(&["ignite/src/lib.rs"]);
    let verb = test_verb(&world, &lane);
    let reply = verb.run(&world.ctx, &request(&world, "test", &["unit:a", "rs:"]));
    assert!(!reply.text.contains("express"), "{}", reply.text);

    let world = express_world(&["ignite/src/lib.rs", "src/main.rs"]);
    let verb = test_verb(&world, &lane);
    let reply = verb.run(&world.ctx, &request(&world, "test", &["unit:a"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(!reply.text.contains("express"), "{}", reply.text);
    assert_eq!(world.launcher.commands("runner")[0][1..], ["unit:a"]);
    // A bare word in a tree with a non-express change still uses the scheduler.
    let reply = verb.run(&world.ctx, &request(&world, "test", &["stuck"]));
    assert!(!reply.text.contains("express"), "{}", reply.text);
    assert!(lane.ran_in.lock().unwrap().is_empty());
}

#[test]
fn a_bare_family_name_takes_the_express_lane() {
    let world = express_world(&["ignite/src/lib.rs"]);
    let lane = Arc::new(FakeLane::default());
    let reply = test_verb(&world, &lane).run(&world.ctx, &request(&world, "test", &["unit"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("in place of the checks of unit"),
        "{}",
        reply.text
    );
    assert_eq!(lane.ran_in.lock().unwrap().len(), 1);
}

#[test]
fn a_bare_test_name_in_an_express_tree_takes_the_express_lane() {
    let world = express_world(&["ignite/src/lib.rs"]);
    let lane = Arc::new(FakeLane::default());
    let reply = test_verb(&world, &lane).run(&world.ctx, &request(&world, "test", &["stuck"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("the whole express command list ran"),
        "{}",
        reply.text
    );
    assert_eq!(lane.ran_in.lock().unwrap().len(), 1);
    assert!(world.launcher.commands("runner").is_empty());
}

#[test]
fn check_in_an_express_tree_runs_the_express_commands_but_plan_does_not() {
    let world = express_world(&["ignite/src/lib.rs"]);
    let lane = Arc::new(FakeLane::default());
    let verb = CheckVerb {
        checks: world.checks.clone(),
        express: Some(lane.clone()),
    };
    let reply = verb.run(&world.ctx, &request(&world, "check", &[]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("in place of the gate's checks"),
        "{}",
        reply.text
    );
    verb.run(&world.ctx, &request(&world, "check", &["--plan"]));
    assert_eq!(lane.ran_in.lock().unwrap().len(), 1);
}
