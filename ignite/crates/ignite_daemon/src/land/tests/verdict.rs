//! Landing by verdict (`[land] mode = "verdict"`): a gate that passed for the same files on the
//! same base lands the change without checking again, however often the tip moves.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use ignite_core::config::LandMode;
use ignite_core::ids::Commit;
use ignite_core::protocol::client::Exit;
use ignite_core::protocol::hub::{Verdict, VerdictKey, verdict_settings};
use ignite_core::traits::{Clock, Git, Hub};

use super::express::{FakeExpress, express_world};
use super::{World, ticket_titled, world, world_with_checks};
use crate::land::records::RecordState;

const TREE: &str = "/trees/tree-01";

fn in_verdict_mode(mut world: World) -> World {
    world.ctx.config.land.mode = LandMode::Verdict;
    world
}

/// The commit the ticket tree has checked out.
fn checked_out(world: &World) -> Commit {
    world.git.repo.lock().unwrap().checked_out[Path::new(TREE)].clone()
}

/// The commit the ticket tree's branch was last rebased onto.
fn base_of(world: &World) -> Commit {
    world.git.repo.lock().unwrap().base[Path::new(TREE)].clone()
}

/// The key of the ticket tree as it is now, on the base `base`.
fn key_now(world: &World, base: &Commit) -> VerdictKey {
    VerdictKey {
        tree: world.git.tree_id(Path::new(TREE)).unwrap(),
        base: base.clone(),
        settings: verdict_settings(&world.ctx.config),
    }
}

/// Stores at the hub a verdict for the ticket tree as it is now, on `base`, as `ignite check`
/// would have.
fn store_verdict(world: &World, base: &Commit) {
    let verdict = Verdict {
        key: key_now(world, base),
        head: checked_out(world),
        machine: "m-1".to_owned(),
        passed_at: world.clock.now(),
        families: vec!["unit".to_owned()],
        left_to_tip: 0,
        always_not_run: Vec::new(),
    };
    world.hub.store_verdict(&verdict).unwrap();
}

/// Queues another landing just before each of the next `n` pushes to the integration branch.
fn tip_moves(world: &World, n: usize) {
    let mut queue = world.git.landed_before_push.lock().unwrap();
    for _ in 0..n {
        queue.push_back(vec![("c.txt", "other")]);
    }
}

#[test]
fn with_a_verdict_a_landing_whose_tip_moves_five_times_runs_no_gate_and_lands() {
    let world = in_verdict_mode(world());
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let base = world.git.tip_now();
    let head = checked_out(&world);
    store_verdict(&world, &base);
    tip_moves(&world, 5);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(world.gate.passes.lock().unwrap().is_empty());
    assert!(world.gate.judged.lock().unwrap().is_empty());
    let pushes = world.git.pushes();
    assert_eq!(pushes.len(), 1);
    // The replayed commit sits on the tip that five other landings made.
    let landed_commit = &pushes[0].1;
    let below = world
        .git
        .repo
        .lock()
        .unwrap()
        .history
        .iter()
        .rev()
        .nth(1)
        .cloned();
    assert_eq!(world.git.parent_of(landed_commit), below);
    assert_eq!(world.landings().len(), 1);
    assert_eq!(
        world.landings()[0].work.commits,
        std::slice::from_ref(landed_commit)
    );
    assert!(
        reply
            .text
            .starts_with("verdict  passed for this tree 0 min ago on"),
        "{}",
        reply.text
    );
    assert!(
        reply
            .text
            .contains("without a checkout: 5 commits landed since"),
        "{}",
        reply.text
    );
    assert_eq!(reply.json.as_ref().unwrap()["passes"], 6);
    // The ticket tree was never rebased or checked out elsewhere, and was released.
    assert_eq!(checked_out(&world), head);
    assert_eq!(base_of(&world), base);
    assert_eq!(world.trees.released.lock().unwrap().len(), 1);
    assert_eq!(world.hub.holder("staging"), None);
}

#[test]
fn without_a_verdict_the_gate_runs_once_in_the_ticket_tree_and_its_verdict_is_stored() {
    let world = in_verdict_mode(world());
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let base = world.git.tip_now();
    world.git.land_other(&[("c.txt", "before")]);
    tip_moves(&world, 2);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let judged = world.gate.judged.lock().unwrap().clone();
    assert_eq!(judged.len(), 1, "{judged:?}");
    assert_eq!(judged[0].0, PathBuf::from(TREE));
    assert!(
        reply.text.contains("verdict  stored for this tree"),
        "{}",
        reply.text
    );
    let stored = world.hub.verdict(&key_now(&world, &base)).unwrap();
    assert!(stored.is_some(), "no verdict stored");
    assert_eq!(world.git.pushes().len(), 1);
    // No landing tree was taken.
    assert!(world.trees.leased.lock().unwrap().is_empty());
}

#[test]
fn a_conflict_in_the_replay_exits_3_and_pushes_nothing() {
    let world = in_verdict_mode(world());
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    store_verdict(&world, &world.git.tip_now());
    world.git.land_other(&[("a.txt", "theirs")]);
    let mut repo = world.git.repo.lock().unwrap();
    repo.conflicts
        .insert(PathBuf::from(TREE), vec!["a.txt".to_owned()]);
    drop(repo);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(
        reply.text.contains("conflicts in 1 files"),
        "{}",
        reply.text
    );
    assert!(reply.text.contains("a.txt"), "{}", reply.text);
    assert!(world.git.pushes().is_empty());
    assert!(world.landings().is_empty());
    assert!(world.landing.records.all().unwrap().is_empty());
    assert_eq!(world.hub.holder("staging"), None);
}

#[test]
fn a_base_older_than_the_limit_rebases_the_ticket_tree_and_gates_again() {
    let world = in_verdict_mode(world());
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let base = world.git.tip_now();
    store_verdict(&world, &base);
    world.clock.advance(13.0 * 3600.0);
    world.git.land_other(&[("c.txt", "later")]);
    let tip = world.git.tip_now();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(base_of(&world), tip);
    let judged = world.gate.judged.lock().unwrap().clone();
    assert_eq!(judged.len(), 1, "{judged:?}");
    assert_eq!(judged[0].0, PathBuf::from(TREE));
    assert_eq!(world.git.pushes().len(), 1);
    assert!(world.hub.verdict(&key_now(&world, &tip)).unwrap().is_some());
}

#[test]
fn a_reverted_ticket_rebases_its_tree_and_gates_there_even_with_a_verdict() {
    let world = in_verdict_mode(world());
    let mut detail = ticket_titled("t.1", "Fix the thing");
    detail.ticket.metadata.insert(
        "reverted".to_owned(),
        serde_json::json!({ "revert": "f".repeat(40) }),
    );
    world.hub.hub.insert_ticket(detail);
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    store_verdict(&world, &world.git.tip_now());
    world.git.land_other(&[("c.txt", "the revert")]);
    let tip = world.git.tip_now();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(base_of(&world), tip);
    let judged = world.gate.judged.lock().unwrap().clone();
    assert_eq!(judged.len(), 1, "{judged:?}");
    assert_eq!(judged[0].0, PathBuf::from(TREE));
    // The gate ran at the rebased commit, which is the one pushed.
    assert_eq!(world.git.pushes()[0].1, checked_out(&world));
}

#[test]
fn a_push_cut_short_leaves_a_pushing_record_that_the_next_landing_settles() {
    let world = in_verdict_mode(world());
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    store_verdict(&world, &world.git.tip_now());
    world.git.land_other(&[("c.txt", "before")]);
    world
        .git
        .push_lands_then_fails
        .store(true, Ordering::SeqCst);

    let reply = world.land_as("tree-01", "local_a");
    assert_eq!(reply.exit, Exit::Unreachable, "{}", reply.text);
    let records = world.landing.records.all().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, RecordState::Pushing);
    let pushed = world.git.tip_now();
    assert_eq!(records[0].commit, pushed);
    assert!(world.landings().is_empty());

    world.git.land_other(&[("c.txt", "after")]);
    let again = world.land_as("tree-01", "local_a");

    assert_eq!(again.exit, Exit::Success, "{}", again.text);
    assert_eq!(world.git.pushes().len(), 1);
    let landed = world.landings();
    assert_eq!(landed.len(), 1);
    assert_eq!(landed[0].work.commits, [pushed]);
    assert!(world.landing.records.all().unwrap().is_empty());
}

#[test]
fn an_ignite_only_change_takes_the_express_lane_and_moves_the_release_in_verdict_mode() {
    let lane = Arc::new(FakeExpress::default());
    let world = in_verdict_mode(express_world(&lane));
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);
    world.git.land_other(&[("c.txt", "before")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("express  1 commands passed"),
        "{}",
        reply.text
    );
    // The ticket tree was rebased onto the tip and the commands ran there.
    let onto = world.git.parent_of(&world.git.tip_now()).unwrap();
    assert_eq!(base_of(&world), onto);
    assert_eq!(*lane.trees.lock().unwrap(), [PathBuf::from(TREE)]);
    let branches = world.git.repo.lock().unwrap().branch_pushes.clone();
    assert_eq!(branches, [("release".to_owned(), world.git.tip_now())]);
}

#[test]
fn a_change_to_ignites_source_and_other_files_lands_checked_with_no_express_section() {
    let mut world = in_verdict_mode(world_with_checks(vec![
        ("unit:a", vec!["a.txt"]),
        ("rel:smoke", vec!["ignite/lib.rs"]),
    ]));
    let repository = &mut world.ctx.config.repository;
    repository.release = Some("release".to_owned());
    repository.release_family = Some("rel".to_owned());
    world.hold(
        "tree-01",
        "t.1",
        "local_a",
        &[("ignite/lib.rs", "new"), ("a.txt", "mine")],
    );
    store_verdict(&world, &world.git.tip_now());
    world.git.land_other(&[("c.txt", "before")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(!reply.text.contains("verdict  "), "{}", reply.text);
    // The gate ran in a landing tree at the rebased commit, which is the one pushed, and the
    // release branch moved to it.
    let judged = world.gate.judged.lock().unwrap().clone();
    assert_eq!(judged.len(), 1, "{judged:?}");
    assert_eq!(judged[0].0, PathBuf::from("/trees/landing-1"));
    let tip = world.git.tip_now();
    assert_eq!(base_of(&world), world.git.parent_of(&tip).unwrap());
    let branches = world.git.repo.lock().unwrap().branch_pushes.clone();
    assert_eq!(branches, [("release".to_owned(), tip)]);
}

#[test]
fn a_change_mixing_ignites_source_with_files_outside_the_express_paths_is_refused() {
    let lane = Arc::new(FakeExpress::default());
    let world = in_verdict_mode(express_world(&lane));
    world.hold(
        "tree-01",
        "t.1",
        "local_a",
        &[("ignite/crates/x.rs", "new"), ("rust/main.rs", "mine")],
    );

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
    assert!(
        reply
            .text
            .starts_with("refused  this change mixes Ignite's own source with other files\n"),
        "{}",
        reply.text
    );
    assert!(
        reply.text.contains("\n         rust/main.rs\n"),
        "{}",
        reply.text
    );
    assert!(!reply.text.contains("ignite/crates/x.rs"), "{}", reply.text);
    assert!(world.gate.judged.lock().unwrap().is_empty());
    assert!(lane.ran.lock().unwrap().is_empty());
    assert!(world.git.pushes().is_empty());
}

#[test]
fn strict_mode_refuses_no_mix_and_lands_it_through_the_gate() {
    let lane = Arc::new(FakeExpress::default());
    let world = express_world(&lane);
    world.hold(
        "tree-01",
        "t.1",
        "local_a",
        &[("ignite/crates/x.rs", "new"), ("rust/main.rs", "mine")],
    );

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(world.git.pushes().len(), 1);
}

#[test]
fn a_check_manifest_with_a_tool_file_is_not_a_mix_and_lands_by_verdict() {
    let lane = Arc::new(FakeExpress::default());
    let mut world = in_verdict_mode(express_world(&lane));
    world.ctx.config.repository.checks = PathBuf::from("ignite/checks");
    world.hold(
        "tree-01",
        "t.1",
        "local_a",
        &[("ignite/checks/x.toml", "new"), ("tools/x.py", "mine")],
    );
    store_verdict(&world, &world.git.tip_now());
    tip_moves(&world, 1);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.starts_with("verdict  passed"), "{}", reply.text);
    assert!(world.gate.judged.lock().unwrap().is_empty());
    assert!(lane.ran.lock().unwrap().is_empty());
    assert_eq!(world.git.pushes().len(), 1);
}

/// A verdict-mode world whose `unit:both` fails wherever `b.txt` says "broken".
fn broken_b() -> World {
    let world = in_verdict_mode(world());
    world
        .gate
        .broken_by
        .lock()
        .unwrap()
        .push(("unit:both", "b.txt", "broken"));
    world
}

/// The gate in the ticket tree fails on a check that fails at the base too. When the base is
/// the tip, the check is filed as a regression and left out, as a strict landing does.
#[test]
fn a_failure_at_a_base_that_is_the_tip_is_filed_and_left_out() {
    let world = broken_b();
    world.git.land_other(&[("b.txt", "broken")]);
    let tip = world.git.tip_now();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let regressions = world.hub.hub.regressions();
    assert_eq!(regressions.len(), 1, "{regressions:?}");
    assert_eq!(regressions[0].first_bad, tip);
    assert_eq!(world.git.pushes().len(), 1);
}

/// When the base is older than the tip and the check is fixed there, the landing rebases the
/// ticket tree onto the tip, the gate passes there, and nothing is filed against the old base.
/// It once refused and said to run `ignite sync`, which rebased onto the same old base, so
/// workers released finished tickets and the pool handed them to new workers.
#[test]
fn a_failure_at_a_base_older_than_the_tip_rebases_and_lands_when_the_tip_fixed_it() {
    let world = broken_b();
    world.git.land_other(&[("b.txt", "broken")]);
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.land_other(&[("b.txt", "fixed")]);
    let tip = world.git.tip_now();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(base_of(&world), tip);
    assert!(world.hub.hub.regressions().is_empty());
    assert_eq!(world.git.pushes().len(), 1);
}

/// When the check still fails at the tip, the landing rebases onto the tip, files the check as a
/// regression there and leaves it out, as a strict landing does.
#[test]
fn a_failure_at_a_base_older_than_the_tip_that_still_fails_there_is_filed_and_left_out() {
    let world = broken_b();
    world.git.land_other(&[("b.txt", "broken")]);
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.land_other(&[("c.txt", "later")]);
    let tip = world.git.tip_now();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(base_of(&world), tip);
    let regressions = world.hub.hub.regressions();
    assert_eq!(regressions.len(), 1, "{regressions:?}");
    assert_eq!(regressions[0].first_bad, tip);
    assert_eq!(world.git.pushes().len(), 1);
}
