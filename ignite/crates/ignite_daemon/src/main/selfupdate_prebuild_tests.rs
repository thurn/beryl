//! Tests of building a landing's release candidate ahead of its landing, and of the sender
//! loop's early rounds (`selfupdate_prebuild.rs`).

use super::*;
use crate::update::prebuild::CANDIDATE_REF;

/// A daemon running `OLD` with the release branch still at `OLD`, whose builds `SlowBuilder`
/// counts (the gate is open), and its selftest.
fn daemon_at_old() -> (
    SelfUpdate,
    Arc<FakeGit>,
    FakeRestart,
    Arc<AtomicUsize>,
    Arc<FakeSelftest>,
) {
    let (builder, gate, started) = SlowBuilder::new(tempdir(), false);
    open_gate(&gate);
    let (mut update, git, restart) = daemon(OLD);
    update.builder = Arc::new(builder);
    let selftest = Arc::new(FakeSelftest::default());
    update.selftest = selftest.clone();
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    // The first round records the release branch's tip it starts from.
    update.poll_release().unwrap();
    settle(&update);
    (update, git, restart, started, selftest)
}

/// An express landing names the commit it is about to check in the candidate ref.
fn name_candidate(git: &FakeGit, hex: &str) {
    git.set_local_ref(Path::new("/tree"), CANDIDATE_REF, &commit(hex))
        .unwrap();
}

#[test]
fn a_landings_candidate_is_built_ahead_and_its_release_only_runs_the_selftest() {
    let (update, git, restart, started, selftest) = daemon_at_old();
    name_candidate(&git, NEW);
    update.poll_release().unwrap();
    settle(&update);

    // Built and stored, but neither tested nor restarted into: it has not landed.
    assert!(update.store.has(&commit(NEW)));
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(selftest.runs.load(Ordering::SeqCst), 0);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.started().is_empty());
    assert_ne!(update.store.current_commit(), Some(commit(NEW)));

    // The landing pushes it: the release only runs its selftest, then restarts.
    git.set_tip("master", commit(NEW));
    update.poll_release().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(selftest.runs.load(Ordering::SeqCst), 1);
}

#[test]
fn a_release_that_lands_while_its_candidate_builds_restarts_once_the_build_ends() {
    let (builder, gate, started) = SlowBuilder::new(tempdir(), false);
    let (mut update, git, restart) = daemon(OLD);
    update.builder = Arc::new(builder);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    update.poll_release().unwrap();

    name_candidate(&git, NEW);
    update.poll_release().unwrap();
    git.set_tip("master", commit(NEW));
    update.poll_release().unwrap();
    assert!(update.building().as_ref().is_some_and(|b| b.prebuild));

    open_gate(&gate);
    let deadline = Instant::now() + Duration::from_secs(30);
    while restart.started().is_empty() {
        assert!(Instant::now() < deadline, "no restart after the build");
        update.after_heartbeat(Some(OLD), IDLE);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_candidate_that_changes_no_ignite_source_or_was_seen_is_not_built() {
    let (update, git, _restart, started, _selftest) = daemon_at_old();
    let docs = "d0c5d0c";
    git.set_changed(&commit(OLD), &commit(docs), &["docs/a.md"]);
    name_candidate(&git, docs);
    update.poll_release().unwrap();
    settle(&update);
    assert!(!update.store.has(&commit(docs)));

    name_candidate(&git, NEW);
    update.poll_release().unwrap();
    settle(&update);
    update.poll_release().unwrap();
    settle(&update);
    assert_eq!(started.load(Ordering::SeqCst), 1);
}

#[test]
fn a_round_is_due_when_the_release_moves_a_candidate_appears_or_a_build_ends() {
    let (update, git, _restart, _started, _selftest) = daemon_at_old();
    assert!(!update.round_due());

    name_candidate(&git, NEW);
    assert!(update.round_due());
    update.poll_release().unwrap();
    // The candidate's build has ended (the gate is open) or is about to: due until collected.
    settle(&update);
    assert!(!update.round_due());

    git.set_tip("master", commit(NEW));
    assert!(update.round_due());
}

#[test]
fn an_unstored_daemon_never_asks_for_an_early_round() {
    let (mut update, git, _restart) = daemon(OLD);
    update.own = None;
    name_candidate(&git, NEW);
    assert!(!update.round_due());
}
