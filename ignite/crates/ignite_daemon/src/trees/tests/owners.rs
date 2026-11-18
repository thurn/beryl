//! Landing trees that record their owner: a restarted daemon keeps a landing tree whose landing
//! it runs again, with the commands still running in it, and the landing run again takes those
//! commands over only while the tree is at the commit they started at.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ignite_core::fakes::FakeLauncher;
use ignite_core::ids::Handle;
use ignite_core::protocol::{Class, Usage};
use ignite_core::time::SystemClock;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};
use ignite_store::local::handles as handle_store;

use super::*;
use crate::adopt::{AdoptingLauncher, HANDLE_VARIABLE, Previous, TREE_COMMIT_VARIABLE};
use crate::steps::{Kind as StepKind, Steps, Work};

fn handle(name: &str) -> Handle {
    Handle::new(name).unwrap()
}

/// Records the handle `id` in the trees' database as `state`, as the server does for a verb.
fn record_handle(f: &Fixture, id: &str, state: &str) {
    handle_store::upsert(
        &f.trees.lock().db,
        &rows::Handle {
            id: id.to_owned(),
            verb: "land".to_owned(),
            session: Some("local_a".to_owned()),
            tree: Some("tree-01".to_owned()),
            state: state.to_owned(),
            exit: None,
            reply: None,
            started_at: "2026-11-18T00:00:00.000Z".to_owned(),
            finished_at: None,
            pid: None,
            executable: None,
            protocol: None,
        },
    )
    .unwrap();
}

fn reset_at_start_now(f: &Fixture) -> Vec<PathBuf> {
    reset_at_start(&f.trees.lock().db).unwrap()
}

/// A file the commands of a landing left in its tree, which a reset would delete.
fn mark(lease: &TreeLease) -> PathBuf {
    let marker = lease.path.join("gate-output.txt");
    fs::write(&marker, "half done\n").unwrap();
    marker
}

fn spec(cwd: &Path) -> CommandSpec {
    CommandSpec {
        args: vec!["cargo".to_owned(), "nextest".to_owned(), "run".to_owned()],
        cwd: cwd.to_path_buf(),
        env: Vec::new(),
        class: Class::Land,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
    }
}

fn finished(exit: i32) -> RunResult {
    RunResult {
        exit: Some(exit),
        output: PathBuf::from("/runs/output.log"),
        usage: Usage::default(),
        hung: false,
    }
}

/// Runs `body` as the verb with handle `owner`.
fn as_handle<T>(owner: &str, body: impl FnOnce() -> T) -> T {
    let steps = Steps::new(Arc::new(SystemClock));
    let activity = steps.begin(StepKind::Verb("land".to_owned()), Work::default());
    activity.attach_handle(&handle(owner));
    let _entered = activity.enter();
    body()
}

/// What a restarted daemon finds of a command the first daemon started for a handle: the
/// command as its launcher was asked to run it, whose supervisor still runs. Its wait finishes
/// with exit 2 and counts in `waits`; its end sets `ended`.
fn left_running(
    started: &CommandSpec,
    waits: &Arc<AtomicUsize>,
    ended: &Arc<AtomicBool>,
) -> Previous {
    let owner = started
        .env
        .iter()
        .find(|(name, _)| name == HANDLE_VARIABLE)
        .map(|(_, value)| handle(value))
        .expect("a handled command carries its handle");
    let (waits, ended) = (waits.clone(), ended.clone());
    Previous {
        handle: owner,
        args: started.args.clone(),
        cwd: started.cwd.clone(),
        env: started.env.clone(),
        wait: Box::new(move || {
            waits.fetch_add(1, Ordering::SeqCst);
            finished(2)
        }),
        end: Box::new(move || ended.store(true, Ordering::SeqCst)),
    }
}

/// A landing `L-1` leases a landing tree at `at` and starts its gate's command there; then the
/// daemon restarts while the command runs. Returns the lease, the command as started, and the
/// marker file its work left in the tree.
fn landing_cut_off_by_a_restart(f: &Fixture, at: &Commit) -> (TreeLease, CommandSpec, PathBuf) {
    record_handle(f, "L-1", "running");
    let first = Arc::new(FakeLauncher::default());
    first.script(&["cargo"], finished(0));
    let adopting = AdoptingLauncher::new(first.clone(), Vec::new());
    let lease = as_handle("L-1", || {
        let lease = f
            .trees
            .lease_landing_for("m:T.1", &handle("L-1"), at)
            .unwrap()
            .expect("a landing tree is free");
        adopting.run(&spec(&lease.path));
        lease
    });
    let marker = mark(&lease);
    let started = first.runs().remove(0);
    (lease, started, marker)
}

#[test]
fn a_landing_tree_records_the_commit_and_owner_of_its_landing() {
    let f = Fixture::new(1, 2);
    let (lease, started, _) = landing_cut_off_by_a_restart(&f, &f.tip());
    assert!(
        started
            .env
            .contains(&(HANDLE_VARIABLE.to_owned(), "L-1".to_owned()))
    );
    assert!(
        started
            .env
            .contains(&(TREE_COMMIT_VARIABLE.to_owned(), f.tip().to_string())),
        "{:?}",
        started.env
    );
    let row = store::get(&f.trees.lock().db, lease.tree.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(row.owner.as_deref(), Some("L-1"));
    assert_eq!(row.ticket.as_deref(), Some("m:T.1"));
}

/// The landing is run again at the same commit: its tree is kept untouched, and the command
/// still running there is taken over, so nothing starts again.
#[test]
fn a_resumed_landing_keeps_its_tree_and_starts_no_command_again() {
    let f = Fixture::new(1, 2);
    let (lease, started, marker) = landing_cut_off_by_a_restart(&f, &f.tip());

    // The restart: the landing's handle is still running, so its tree is neither reset nor
    // are its commands ended.
    assert!(!reset_at_start_now(&f).contains(&lease.path));
    f.trees.setup(&f.commits[0]).unwrap();
    assert_eq!(f.state(lease.tree.as_str()), "landing");
    assert!(marker.exists(), "setup reset the kept tree");

    let (waits, ended) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicBool::new(false)),
    );
    let fresh = Arc::new(FakeLauncher::default());
    fresh.script(&["cargo"], finished(0));
    let second = AdoptingLauncher::new(fresh.clone(), vec![left_running(&started, &waits, &ended)]);
    let (again, result) = as_handle("L-1", || {
        let again = f
            .trees
            .lease_landing_for("m:T.1", &handle("L-1"), &f.tip())
            .unwrap()
            .expect("the landing gets its tree back");
        let result = second.run(&spec(&again.path));
        (again, result)
    });
    assert_eq!(again.tree, lease.tree);
    assert!(
        marker.exists(),
        "the lease reset a tree already at the commit"
    );
    assert_eq!(result.exit, Some(2), "the running command's own result");
    assert_eq!(waits.load(Ordering::SeqCst), 1);
    assert!(fresh.runs().is_empty(), "{:?}", fresh.runs());
    assert!(!ended.load(Ordering::SeqCst));
}

/// The landing is run again but rebased onto a newer tip: its tree is reset to the new commit,
/// and the command that checked the older one is ended and run again, never taken over.
#[test]
fn a_command_that_ran_at_another_commit_is_ended_and_run_again() {
    let f = Fixture::new(1, 2);
    let (lease, started, marker) = landing_cut_off_by_a_restart(&f, &f.commits[0]);
    f.trees.setup(&f.commits[0]).unwrap();

    let (waits, ended) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicBool::new(false)),
    );
    let fresh = Arc::new(FakeLauncher::default());
    fresh.script(&["cargo"], finished(0));
    let second = AdoptingLauncher::new(fresh.clone(), vec![left_running(&started, &waits, &ended)]);
    let result = as_handle("L-1", || {
        let again = f
            .trees
            .lease_landing_for("m:T.1", &handle("L-1"), &f.tip())
            .unwrap()
            .expect("the landing gets its tree back");
        assert_eq!(again.tree, lease.tree);
        assert_eq!(head::commit_at(&again.path), Some(f.tip()));
        second.run(&spec(&again.path))
    });
    assert!(!marker.exists(), "a tree at another commit is reset");
    assert_eq!(result.exit, Some(0), "the command ran again");
    assert_eq!(
        waits.load(Ordering::SeqCst),
        0,
        "the old command was taken over"
    );
    assert!(
        ended.load(Ordering::SeqCst),
        "the old command was left running"
    );
    let runs = fresh.runs();
    assert_eq!(runs.len(), 1);
    assert!(
        runs[0]
            .env
            .contains(&(TREE_COMMIT_VARIABLE.to_owned(), f.tip().to_string()))
    );
}

/// A landing tree whose owner finished, or that has no owner (tip verification), is reset and
/// freed at start as before, and its commands are ended.
#[test]
fn a_landing_tree_whose_owner_does_not_run_again_is_reset_at_start() {
    let f = Fixture::with_landing(1, 2, 1);
    let (owned, _, marker) = landing_cut_off_by_a_restart(&f, &f.tip());
    let tip_verification = f
        .trees
        .lease_landing("tip verification", &f.tip())
        .unwrap()
        .unwrap();
    record_handle(&f, "L-1", "done");

    let reset = reset_at_start_now(&f);
    assert!(reset.contains(&owned.path), "{reset:?}");
    assert!(reset.contains(&tip_verification.path), "{reset:?}");
    f.trees.setup(&f.tip()).unwrap();
    assert_eq!(f.state(owned.tree.as_str()), "free");
    assert_eq!(f.state(tip_verification.tree.as_str()), "free");
    assert!(!marker.exists());
    let row = store::get(&f.trees.lock().db, owned.tree.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(row.owner, None);
}

/// A landing tree kept for a verb that then finished without freeing it (ended `incomplete`, or
/// run again down a path that needs no landing tree) is freed by the next landing lease.
#[test]
fn a_tree_kept_for_an_owner_that_finished_is_freed_by_the_next_lease() {
    let f = Fixture::new(1, 1);
    let (owned, _, _) = landing_cut_off_by_a_restart(&f, &f.tip());
    f.trees.setup(&f.tip()).unwrap();
    assert_eq!(f.state(owned.tree.as_str()), "landing");
    record_handle(&f, "L-1", "incomplete");

    let next = f
        .trees
        .lease_landing("tip verification", &f.tip())
        .unwrap()
        .expect("the finished landing's tree is freed");
    assert_eq!(next.tree, owned.tree);
}

/// A live owner's tree is never given to anyone else.
#[test]
fn a_running_owners_tree_is_not_given_to_another_holder() {
    let f = Fixture::new(1, 1);
    landing_cut_off_by_a_restart(&f, &f.tip());
    assert!(
        f.trees
            .lease_landing("tip verification", &f.tip())
            .unwrap()
            .is_none()
    );
    record_handle(&f, "L-2", "running");
    assert!(
        f.trees
            .lease_landing_for("m:T.2", &handle("L-2"), &f.tip())
            .unwrap()
            .is_none()
    );
}
