//! More landing tests: refused pushes, a lost lease, settling `pushing` records and moving the
//! release branch.

use std::sync::atomic::Ordering;

use ignite_core::ids::{Commit, SessionId, TicketId, TreeId};
use ignite_core::protocol::client::Exit;
use ignite_core::protocol::hub::Landed;
use ignite_core::protocol::tickets::LandedWork;
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;

use super::fakes::TestGit;
use super::{World, world, world_with, world_with_checks};
use crate::land::env::LandTrees as _;
use crate::land::records::{LandingRecord, RecordState};

/// Writes a `pushing` record of the held ticket's, for `commit`, written at `at`.
fn write_pushing(world: &World, commit: &Commit, at: Option<Timestamp>) -> LandingRecord {
    let held = world.trees.tickets.lock().unwrap()[&TreeId::new("tree-01").unwrap()].clone();
    let record = LandingRecord {
        landed: Landed {
            ticket: held.ticket.clone(),
            machine: world.ctx.machine.clone(),
            session: held.session.clone().unwrap(),
            work: LandedWork {
                commits: vec![commit.clone()],
                files: Vec::new(),
                note: "the note".to_owned(),
            },
            fresh: false,
        },
        commit: commit.clone(),
        state: RecordState::Pushing,
        written_at: at,
        tree: Some(TreeId::new("tree-01").unwrap()),
    };
    world.landing.records.write(&record).unwrap();
    record
}

#[test]
fn a_refused_push_leaves_no_record_and_releases_the_lease() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.push_refused.store(true, Ordering::SeqCst);
    let reply = world.land_as("tree-01", "local_a");
    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(world.landing.records.all().unwrap().is_empty());
    assert_eq!(world.hub.holder("staging"), None);
}

#[test]
fn removing_a_record_for_another_commit_leaves_it_alone() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let record = write_pushing(&world, &Commit::new("a".repeat(40)).unwrap(), None);
    let other = Commit::new("b".repeat(40)).unwrap();

    world
        .landing
        .records
        .remove_if(&record.landed.ticket, &other)
        .unwrap();
    assert_eq!(world.landing.records.all().unwrap().len(), 1);
    world
        .landing
        .records
        .remove_if(&record.landed.ticket, &record.commit)
        .unwrap();
    assert!(world.landing.records.all().unwrap().is_empty());
}

#[test]
fn a_lease_taken_over_before_the_push_makes_the_landing_start_again() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.hub.steal_after_acquire.store(true, Ordering::SeqCst);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("landed on pass 2"), "{}", reply.text);
    // It pushed once, and only with a lease of its own; the thief's lease was left alone.
    assert_eq!(world.git.pushes().len(), 1);
    assert_eq!(world.hub.holder("staging"), None);
}

#[test]
fn the_sender_closes_a_ticket_whose_push_landed_but_whose_landing_never_ran_again() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world
        .git
        .push_lands_then_fails
        .store(true, Ordering::SeqCst);
    let reply = world.land_as("tree-01", "local_a");
    assert_eq!(reply.exit, Exit::Unreachable, "{}", reply.text);

    // A record that is still young is left to its landing.
    world.landing.deliver_pending(&world.ctx);
    assert!(world.landings().is_empty());

    world.clock.advance(120.0);
    world.landing.deliver_pending(&world.ctx);

    assert_eq!(world.landings().len(), 1);
    assert!(world.landing.records.all().unwrap().is_empty());
    assert_eq!(world.git.pushes().len(), 1);
}

#[test]
fn a_landing_the_sender_settles_releases_its_ticket_tree() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world
        .git
        .push_lands_then_fails
        .store(true, Ordering::SeqCst);
    world.land_as("tree-01", "local_a");
    assert!(world.trees.released.lock().unwrap().is_empty());

    world.clock.advance(120.0);
    world.landing.deliver_pending(&world.ctx);

    let tree = TreeId::new("tree-01").unwrap();
    assert_eq!(
        *world.trees.released.lock().unwrap(),
        std::slice::from_ref(&tree)
    );
    assert_eq!(world.trees.deleted.lock().unwrap().len(), 1);
    assert!(world.trees.ticket_tree(&tree).unwrap().is_none());
}

#[test]
fn a_settled_landing_leaves_a_tree_leased_to_another_ticket_alone() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world
        .git
        .push_lands_then_fails
        .store(true, Ordering::SeqCst);
    world.land_as("tree-01", "local_a");
    // The ticket was released by hand and its tree given to another ticket meanwhile.
    {
        let mut tickets = world.trees.tickets.lock().unwrap();
        let held = tickets.get_mut(&TreeId::new("tree-01").unwrap()).unwrap();
        held.ticket = TicketId::new("t.2").unwrap();
        held.session = Some(SessionId::new("local_b").unwrap());
    }

    world.clock.advance(120.0);
    world.landing.deliver_pending(&world.ctx);

    assert_eq!(world.landings().len(), 1);
    assert!(world.trees.released.lock().unwrap().is_empty());
}

#[test]
fn the_sender_drops_an_old_record_whose_commit_never_reached_the_remote() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let never = Commit::new("f".repeat(40)).unwrap();
    write_pushing(&world, &never, Some(world.clock.now()));

    world.clock.advance(120.0);
    world.landing.deliver_pending(&world.ctx);
    // Not on the remote, but not old enough to be sure the push is not still in flight.
    assert_eq!(world.landing.records.all().unwrap().len(), 1);

    world.clock.advance(600.0);
    world.landing.deliver_pending(&world.ctx);
    assert!(world.landing.records.all().unwrap().is_empty());
    assert!(world.landings().is_empty());
}

fn with_release(world: &mut World) {
    let repository = &mut world.ctx.config.repository;
    repository.release = Some("release".to_owned());
    repository.release_family = Some("rel".to_owned());
}

fn source_world() -> World {
    let mut world = world_with_checks(vec![
        ("unit:a", vec!["a.txt"]),
        ("rel:smoke", vec!["ignite/lib.rs"]),
    ]);
    with_release(&mut world);
    world
}

fn released(git: &TestGit) -> Vec<String> {
    git.repo
        .lock()
        .unwrap()
        .branch_pushes
        .iter()
        .map(|(branch, _)| branch.clone())
        .collect()
}

#[test]
fn a_landing_that_changed_ignite_moves_the_release_branch_after_the_gate_ran_its_family() {
    let world = source_world();
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("release  release moved to"),
        "{}",
        reply.text
    );
    assert_eq!(released(&world.git), ["release"]);
    let pushed = world.git.repo.lock().unwrap().branch_pushes[0].1.clone();
    assert!(
        reply.text.contains(&format!(
            "next     ignite wait deploy-{pushed} until every machine"
        )),
        "{}",
        reply.text
    );
    // The gate's own run counted, so the family was not run a second time.
    assert!(world.gate.family_runs.lock().unwrap().is_empty());
}

#[test]
fn the_release_family_is_run_when_the_gate_did_not_include_it() {
    // The gate has no `rel` checks, so the landing must run them itself.
    let mut world = world_with(vec![("unit:a", vec!["a.txt"])], vec!["rel:smoke"]);
    with_release(&mut world);
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(*world.gate.family_runs.lock().unwrap(), ["rel"]);
    assert_eq!(released(&world.git), ["release"]);
}

#[test]
fn a_failing_release_family_leaves_the_branch_and_does_not_fail_the_landing() {
    let mut world = world_with(vec![("unit:a", vec!["a.txt"])], vec!["rel:smoke"]);
    with_release(&mut world);
    world
        .gate
        .failing
        .lock()
        .unwrap()
        .insert("rel:smoke".to_owned());
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("release  release not moved"),
        "{}",
        reply.text
    );
    assert!(released(&world.git).is_empty());
    assert_eq!(world.landings().len(), 1);
}

#[test]
fn a_rejected_release_push_is_reported_and_the_landing_stands() {
    let world = source_world();
    world
        .git
        .other_branch_rejected
        .store(true, Ordering::SeqCst);
    // The release branch holds a commit whose changes the landing lacks.
    let foreign = Commit::new("f".repeat(40)).unwrap();
    world
        .git
        .repo
        .lock()
        .unwrap()
        .other_tips
        .insert("release".to_owned(), foreign);
    world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("release  release not moved"),
        "{}",
        reply.text
    );
    assert_eq!(world.landings().len(), 1);
}

#[test]
fn a_landing_outside_ignites_source_or_without_a_release_branch_leaves_it_alone() {
    let world = source_world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let reply = world.land_as("tree-01", "local_a");
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(!reply.text.contains("release  "), "{}", reply.text);
    assert!(released(&world.git).is_empty());

    let plain = world_with_checks(vec![("rel:smoke", vec!["ignite/lib.rs"])]);
    plain.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);
    let reply = plain.land_as("tree-01", "local_a");
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(released(&plain.git).is_empty());
}

/// An activity for a landing (see `crate::steps`), entered on the test's thread.
fn landing_activity() -> std::sync::Arc<crate::steps::Activity> {
    let steps = crate::steps::Steps::new(std::sync::Arc::new(ignite_core::time::SystemClock));
    steps.begin(
        crate::steps::Kind::Verb("land".to_owned()),
        crate::steps::Work::default(),
    )
}

#[test]
fn a_landing_stopped_during_its_gate_pushes_nothing_and_gives_back_its_tree_and_lease() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.land_other(&[("c.txt", "meanwhile")]);
    let activity = landing_activity();
    let _entered = activity.enter();
    let asked = activity.clone();
    world.queue_hook(move || asked.request_stop().unwrap());

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(world.git.pushes().is_empty());
    assert!(world.landings().is_empty());
    assert_eq!(world.hub.holder("staging"), None);
    assert_eq!(
        world.trees.landing.lock().unwrap().len(),
        1,
        "the landing tree is free"
    );
    assert!(world.landing.records.all().unwrap().is_empty());
    let work = activity.work();
    assert_eq!(work.class, Some(ignite_core::protocol::Class::Land));
    assert_eq!(work.ticket.map(|t| t.to_string()).as_deref(), Some("t.1"));
}

#[test]
fn a_landing_refuses_to_stop_once_its_push_has_begun() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let activity = landing_activity();
    let _entered = activity.enter();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        activity.request_stop(),
        Err("its push to staging has begun".to_owned())
    );
    assert!(
        activity
            .view()
            .step
            .is_some_and(|step| step.starts_with("pushing ")),
        "{:?}",
        activity.view()
    );
}

#[test]
fn a_landing_whose_push_was_rejected_can_be_stopped_on_its_next_pass() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    // Another machine lands a file a check reads just before the first push, which is
    // rejected, so the second pass gates again.
    world
        .git
        .landed_before_push
        .lock()
        .unwrap()
        .push_back(vec![("c.txt", "raced")]);
    let activity = landing_activity();
    let _entered = activity.enter();
    // The first pass's gate does nothing; the second pass's, after the rejected push, asks the
    // landing to stop.
    world.queue_hook(|| {});
    let asked = activity.clone();
    let answer = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = answer.clone();
    world.queue_hook(move || *slot.lock().unwrap() = Some(asked.request_stop()));

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(*answer.lock().unwrap(), Some(Ok(())));
    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert_eq!(world.gate.passes.lock().unwrap().len(), 2, "{}", reply.text);
    assert!(world.git.pushes().is_empty());
    assert_eq!(world.hub.holder("staging"), None);
}

/// The deploy handle is named only for a `release` line that says the branch moved to the
/// landed commit.
#[test]
fn a_landing_that_moved_the_release_names_its_deploy_handle() {
    use crate::deploy::landed_handle;
    let world = source_world();
    let reply = {
        world.hold("tree-01", "t.1", "local_a", &[("ignite/lib.rs", "new")]);
        world.land_as("tree-01", "local_a")
    };
    let line = reply
        .text
        .lines()
        .find_map(|line| line.strip_prefix("release  "))
        .unwrap();
    let pushed = world.git.repo.lock().unwrap().branch_pushes[0].1.clone();
    let branch = world.ctx.config.repository.release.as_deref();
    assert_eq!(
        landed_handle(branch, line, &pushed),
        Some(format!("deploy-{pushed}"))
    );
    let refused = "release not moved: rel checks did not pass";
    assert_eq!(landed_handle(branch, refused, &pushed), None);
    assert_eq!(landed_handle(None, line, &pushed), None);
}
