use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::fakes::FakeGit;

use super::*;

const OLD: &str = "a1b2c3d";
const NEW: &str = "e4f5a6b";

fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

fn tempdir() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ignite-selfupdate-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a stand-in executable for each commit it is asked to build.
struct FakeBuilder {
    out: PathBuf,
}

impl Builder for FakeBuilder {
    fn build(&self, commit: &Commit) -> Result<PathBuf> {
        let path = self.out.join(format!("built-{commit}"));
        std::fs::write(&path, commit.as_str())?;
        Ok(path)
    }
}

/// Records each restart instead of replacing the test's process.
#[derive(Clone, Default)]
struct FakeRestart {
    started: Arc<Mutex<Vec<PathBuf>>>,
}

impl Restart for FakeRestart {
    fn restart(&self, executable: &Path, _args: &[String]) -> io::Result<()> {
        self.started.lock().unwrap().push(executable.to_owned());
        Ok(())
    }
}

impl FakeRestart {
    fn started(&self) -> Vec<PathBuf> {
        self.started.lock().unwrap().clone()
    }
}

/// A daemon running `OLD`, with `master` at `master`.
fn daemon(master: &str) -> (SelfUpdate, Arc<FakeGit>, FakeRestart) {
    let dir = tempdir();
    let git = Arc::new(FakeGit::default());
    git.set_tip("master", commit(master));
    let restart = FakeRestart::default();
    let update = SelfUpdate {
        own: Some(commit(OLD)),
        git: git.clone(),
        builder: Box::new(FakeBuilder { out: dir.clone() }),
        store: ExecutableStore::new(dir.join("bin")),
        master: "master".to_owned(),
        restart: Box::new(restart.clone()),
        ready: Mutex::new(None),
        rounds: AtomicU32::new(0),
        ahead_of: Mutex::default(),
    };
    (update, git, restart)
}

#[test]
fn a_daemon_on_another_version_than_the_hub_restarts_into_the_hubs_at_once() {
    let (update, _git, restart) = daemon(NEW);

    // Commands may run; the restart does not wait for them.
    update.after_heartbeat(Some(NEW), false);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_daemon_on_the_hubs_version_stays() {
    let (update, _git, restart) = daemon(OLD);
    update.after_heartbeat(Some(OLD), false);
    // A hub that has not answered yet changes nothing.
    update.after_heartbeat(None, false);
    assert!(restart.started().is_empty());
}

#[test]
fn master_moving_with_changed_ignite_source_builds_and_restarts_at_once() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);

    update.after_master_moved().unwrap();
    assert!(update.store.has(&commit(NEW)));

    // The hub is still on this daemon's version; the new build is restarted into at the next
    // heartbeat, whatever runs.
    update.after_heartbeat(Some(OLD), false);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_ready_build_waits_only_for_a_push_in_progress() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    update.after_master_moved().unwrap();

    update.after_heartbeat(Some(OLD), true);
    assert!(restart.started().is_empty());
    update.after_heartbeat(Some(OLD), false);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn master_moving_without_ignite_changes_keeps_the_executable() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["docs/readme.md"]);

    update.after_master_moved().unwrap();
    update.after_heartbeat(Some(OLD), false);
    assert!(restart.started().is_empty());
    assert!(!update.store.has(&commit(NEW)));
}

#[test]
fn a_daemon_already_on_masters_tip_waits_for_the_hub_to_catch_up() {
    // This daemon runs `OLD`, which is `master`'s tip; the hub still reports an older commit.
    let (update, _git, restart) = daemon(OLD);
    update.after_heartbeat(Some(NEW), false);
    assert!(restart.started().is_empty());
}

#[test]
fn an_unstored_daemon_never_updates() {
    let (mut update, _git, restart) = daemon(NEW);
    update.own = None;
    update.after_heartbeat(Some(NEW), false);
    update.after_master_moved().unwrap();
    assert!(restart.started().is_empty());
}

/// Calls `poll_release` until it does its check, which is every `POLL_EVERY_ROUNDS`th call.
fn poll(update: &SelfUpdate) {
    update.rounds.store(0, Ordering::Relaxed);
    update.poll_release().unwrap();
}

#[test]
fn a_push_straight_to_the_release_branch_deploys() {
    let (mut update, git, restart) = daemon(OLD);
    update.master = "release".to_owned();
    git.set_tip("release", commit(NEW));
    git.set_changed(
        &commit(OLD),
        &commit(NEW),
        &["ignite/crates/x.rs", "docs/a.md"],
    );

    poll(&update);
    assert!(update.store.has(&commit(NEW)));
    update.after_heartbeat(Some(OLD), false);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_release_branch_that_has_not_moved_builds_nothing() {
    let (update, _git, restart) = daemon(OLD);
    poll(&update);
    update.after_heartbeat(Some(OLD), false);
    assert!(restart.started().is_empty());
    assert!(!update.store.has(&commit(NEW)));
}

#[test]
fn a_release_move_without_ignite_changes_builds_nothing() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["docs/readme.md"]);
    poll(&update);
    update.after_heartbeat(Some(OLD), false);
    assert!(restart.started().is_empty());
    assert!(!update.store.has(&commit(NEW)));
}

#[test]
fn a_build_recorded_as_bad_is_never_installed_again_but_a_newer_one_is() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/a.rs"]);
    update.store.reject(&commit(NEW));

    poll(&update);
    assert!(!update.store.has(&commit(NEW)));
    // Not by the hub's version either.
    update.after_heartbeat(Some(NEW), false);
    assert!(restart.started().is_empty());

    let newer = "f00dfee";
    git.set_tip("master", commit(newer));
    git.set_changed(&commit(OLD), &commit(newer), &["ignite/a.rs"]);
    poll(&update);
    assert!(update.store.has(&commit(newer)));
}

#[test]
fn the_release_branch_is_fetched_at_most_every_thirty_rounds() {
    let (update, git, _restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/a.rs"]);
    update.rounds.store(1, Ordering::Relaxed);
    for _ in 1..POLL_EVERY_ROUNDS {
        update.poll_release().unwrap();
    }
    assert!(!update.store.has(&commit(NEW)));
    update.poll_release().unwrap();
    assert!(update.store.has(&commit(NEW)));
}

#[test]
fn a_daemon_installed_ahead_of_the_release_branch_stays_on_its_build() {
    // Installed by hand from a newer commit: `master`'s tip differs from the daemon's commit
    // and Ignite's source differs between them, but the tip does not contain the daemon's
    // commit, so going there would be a downgrade.
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    git.set_not_ancestor(&commit(OLD), &commit(NEW));

    update.poll_release().unwrap();
    assert!(!update.store.has(&commit(NEW)));
    // The daemon serves the hub, so the hub reports the daemon's own version.
    update.after_heartbeat(Some(OLD), false);
    assert!(restart.started().is_empty());
}
