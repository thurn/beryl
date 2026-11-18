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
    };
    (update, git, restart)
}

#[test]
fn a_daemon_on_another_version_than_the_hub_drains_then_restarts_into_the_hubs() {
    let (update, _git, restart) = daemon(NEW);

    // Two commands still run: no new work, no restart.
    assert!(!update.after_heartbeat(Some(NEW), 2));
    assert!(restart.started().is_empty());

    // Once nothing runs, the hub's version is built and the daemon restarts into it.
    assert!(!update.after_heartbeat(Some(NEW), 0));
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_daemon_on_the_hubs_version_asks_for_work() {
    let (update, _git, restart) = daemon(OLD);
    assert!(update.after_heartbeat(Some(OLD), 3));
    // A hub that has not answered yet changes nothing.
    assert!(update.after_heartbeat(None, 0));
    assert!(restart.started().is_empty());
}

#[test]
fn master_moving_with_changed_ignite_source_builds_and_restarts_once_idle() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);

    update.after_master_moved().unwrap();
    assert!(update.store.has(&commit(NEW)));

    // The hub is still on this daemon's version, but the new build waits for the running
    // command and the daemon takes no new work meanwhile.
    assert!(!update.after_heartbeat(Some(OLD), 1));
    assert!(restart.started().is_empty());

    assert!(!update.after_heartbeat(Some(OLD), 0));
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn master_moving_without_ignite_changes_keeps_the_executable() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["docs/readme.md"]);

    update.after_master_moved().unwrap();
    assert!(update.after_heartbeat(Some(OLD), 0));
    assert!(restart.started().is_empty());
    assert!(!update.store.has(&commit(NEW)));
}

#[test]
fn a_daemon_already_on_masters_tip_waits_for_the_hub_to_catch_up() {
    // This daemon runs `OLD`, which is `master`'s tip; the hub still reports an older commit.
    let (update, _git, restart) = daemon(OLD);
    assert!(!update.after_heartbeat(Some(NEW), 0));
    assert!(restart.started().is_empty());
}

#[test]
fn an_unstored_daemon_never_updates() {
    let (mut update, _git, restart) = daemon(NEW);
    update.own = None;
    assert!(update.after_heartbeat(Some(NEW), 0));
    update.after_master_moved().unwrap();
    assert!(restart.started().is_empty());
}
