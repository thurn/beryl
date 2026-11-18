use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex as StdMutex};
use std::time::{Duration, Instant};

use ignite_core::fakes::{FakeClock, FakeGit, FakeHub};
use ignite_core::ids::MachineId;

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

/// A selftest that passes except for the commits it is told to fail, and counts its runs. It
/// finds the commit from the executable's folder, as the real one is stored by commit.
#[derive(Default)]
struct FakeSelftest {
    fails: Mutex<Vec<String>>,
    runs: AtomicUsize,
}

impl Selftest for FakeSelftest {
    fn run(&self, executable: &Path) -> Result<String> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let commit = executable
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        if self.fails.lock().unwrap().iter().any(|c| *c == commit) {
            return Err(Error::Io(format!(
                "selftest: cannot parse the manifest of {commit}"
            )));
        }
        Ok("selftest passed".to_owned())
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
        builder: Arc::new(FakeBuilder { out: dir.clone() }),
        store: ExecutableStore::new(dir.join("bin")),
        master: "master".to_owned(),
        restart: Box::new(restart.clone()),
        ready: Mutex::new(None),
        rounds: AtomicU32::new(0),
        ahead_of: Mutex::default(),
        building: Mutex::default(),
        failed: Mutex::default(),
        selftest: Arc::new(FakeSelftest::default()),
        hub: Arc::new(FakeHub::default()),
        clock: Arc::new(FakeClock::new(ignite_core::time::Timestamp::from_millis(0))),
        machine: MachineId::new("m-1").unwrap(),
        host: "jarvis".to_owned(),
        reports: Mutex::default(),
    };
    (update, git, restart)
}

#[test]
fn a_daemon_on_another_version_than_the_hub_restarts_into_the_hubs_at_once() {
    let (update, _git, restart) = daemon(NEW);

    // Commands may run; the restart does not wait for them. The hub's version is built on a
    // thread of its own, and the heartbeat after it ends restarts into it.
    update.after_heartbeat(Some(NEW), false);
    settle(&update);
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
    settle(&update);
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
    settle(&update);

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
    settle(&update);
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

/// Waits for the release build thread to end and takes its result, as the sender's next rounds
/// do. Fails the test if the build does not end.
fn settle(update: &SelfUpdate) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        update.collect_build();
        if update.building().is_none() {
            return;
        }
        assert!(Instant::now() < deadline, "the release build never ended");
        thread::sleep(Duration::from_millis(2));
    }
}

/// Calls `poll_release` until it does its check, which is every `POLL_EVERY_ROUNDS`th call.
fn poll(update: &SelfUpdate) {
    update.rounds.store(0, Ordering::Relaxed);
    update.poll_release().unwrap();
    settle(update);
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
    settle(&update);
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

/// A builder that does not finish until the test opens its gate, and counts its builds. Each
/// build ends as the test says: with a stored executable, or with an error.
/// The gate a [`SlowBuilder`] waits at: open once the flag is true.
type Gate = Arc<(StdMutex<bool>, Condvar)>;

struct SlowBuilder {
    out: PathBuf,
    open: Gate,
    started: Arc<AtomicUsize>,
    fails: bool,
}

impl SlowBuilder {
    fn new(out: PathBuf, fails: bool) -> (Self, Gate, Arc<AtomicUsize>) {
        let open = Arc::new((StdMutex::new(false), Condvar::new()));
        let started = Arc::new(AtomicUsize::new(0));
        let builder = SlowBuilder {
            out,
            open: open.clone(),
            started: started.clone(),
            fails,
        };
        (builder, open, started)
    }
}

impl Builder for SlowBuilder {
    fn build(&self, commit: &Commit) -> Result<PathBuf> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let (lock, wake) = &*self.open;
        let mut open = lock.lock().unwrap();
        while !*open {
            open = wake.wait(open).unwrap();
        }
        if self.fails {
            return Err(Error::Io(format!("cargo build failed for {commit}")));
        }
        let path = self.out.join(format!("built-{commit}"));
        std::fs::write(&path, commit.as_str())?;
        Ok(path)
    }
}

fn open_gate(gate: &(StdMutex<bool>, Condvar)) {
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}

/// A daemon, as [`daemon`] makes it, whose builds are `builder`.
fn daemon_building_with(builder: SlowBuilder) -> (SelfUpdate, Arc<FakeGit>, FakeRestart) {
    let (mut update, git, restart) = daemon(NEW);
    update.builder = Arc::new(builder);
    (update, git, restart)
}

#[test]
fn heartbeats_go_on_while_a_slow_release_builds_and_the_build_starts_once() {
    let (builder, gate, started) = SlowBuilder::new(tempdir(), false);
    let (update, git, restart) = daemon_building_with(builder);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);

    // One round of the sender loop, as `spawn_sender` runs it: the check, then the heartbeat.
    // Each round counts as a heartbeat sent. The tip loop asks for the same build as well, and
    // the hub reports the new version, which is a third request.
    let heartbeats = std::cell::Cell::new(0);
    let round = |update: &SelfUpdate| {
        update.rounds.store(0, Ordering::Relaxed);
        update.poll_release().unwrap();
        update.after_heartbeat(Some(NEW), false);
        heartbeats.set(heartbeats.get() + 1);
    };
    let began = Instant::now();
    for _ in 0..5 {
        round(&update);
        update.after_master_moved().unwrap();
    }
    // The rounds returned while the build was still gated: nothing blocked on it.
    assert_eq!(heartbeats.get(), 5);
    assert!(began.elapsed() < Duration::from_secs(20));
    assert!(restart.started().is_empty());
    assert_eq!(
        update.building().as_ref().map(|b| b.commit.clone()),
        Some(commit(NEW))
    );
    assert!(!update.store.has(&commit(NEW)));

    open_gate(&gate);
    let deadline = Instant::now() + Duration::from_secs(30);
    while restart.started().is_empty() {
        assert!(
            Instant::now() < deadline,
            "no restart after the build ended"
        );
        round(&update);
        thread::sleep(Duration::from_millis(2));
    }
    // The build ran once however many rounds and loops asked, and the first heartbeat after
    // it ended restarted the daemon into it.
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_failed_release_build_is_not_started_again_for_the_same_commit() {
    let (builder, gate, started) = SlowBuilder::new(tempdir(), true);
    let (update, git, restart) = daemon_building_with(builder);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/a.rs"]);
    open_gate(&gate);

    poll(&update);
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(update.failed().as_ref(), Some(&commit(NEW)));

    // Neither the sender's poll, the tip loop nor the hub's version tries the same commit again.
    poll(&update);
    update.after_master_moved().unwrap();
    update.after_heartbeat(Some(NEW), false);
    settle(&update);
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert!(restart.started().is_empty());

    // The release branch moving to another commit builds that one.
    let newer = "f00dfee";
    git.set_tip("master", commit(newer));
    git.set_changed(&commit(OLD), &commit(newer), &["ignite/a.rs"]);
    poll(&update);
    assert_eq!(started.load(Ordering::SeqCst), 2);
}

/// A daemon, as [`daemon`] makes it, with the fake hub and selftest it was given.
fn daemon_with_selftest() -> (
    SelfUpdate,
    Arc<FakeGit>,
    FakeRestart,
    Arc<FakeHub>,
    Arc<FakeSelftest>,
) {
    let (mut update, git, restart) = daemon(NEW);
    let hub = Arc::new(FakeHub::default());
    let selftest = Arc::new(FakeSelftest::default());
    update.hub = hub.clone();
    update.selftest = selftest.clone();
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    (update, git, restart, hub, selftest)
}

#[test]
fn a_release_that_passes_its_selftest_restarts_and_is_checked_once() {
    let (update, _git, restart, hub, selftest) = daemon_with_selftest();

    // The hub's version is asked for on every heartbeat, and the tip loop asks too; one selftest
    // runs however often.
    update.after_heartbeat(Some(NEW), false);
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(NEW), false);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    assert_eq!(selftest.runs.load(Ordering::SeqCst), 1);
    assert!(hub.justiciar_reports().is_empty());
    assert_eq!(update.store.current_commit(), Some(commit(NEW)));
}

#[test]
fn a_release_that_fails_its_selftest_is_never_restarted_into_and_is_reported_once() {
    let (update, git, restart, hub, selftest) = daemon_with_selftest();
    selftest.fails.lock().unwrap().push(NEW.to_owned());

    // From the tip loop, the poll and the hub's version alike.
    update.after_master_moved().unwrap();
    settle(&update);
    for _ in 0..3 {
        poll(&update);
        update.after_heartbeat(Some(NEW), false);
        update.after_heartbeat(Some(OLD), false);
        settle(&update);
    }

    assert!(restart.started().is_empty());
    assert_eq!(selftest.runs.load(Ordering::SeqCst), 1);
    assert!(update.store.is_rejected(&commit(NEW)));
    // `bin/current` was never pointed at it.
    assert_eq!(update.store.current_commit(), None);
    let reports = hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].details.contains("cannot parse the manifest"));
    assert_eq!(reports[0].subject, format!("release-selftest:{NEW}"));

    // A newer release that passes is restarted into.
    let newer = "f00dfee";
    git.set_tip("master", commit(newer));
    git.set_changed(&commit(OLD), &commit(newer), &["ignite/crates/x.rs"]);
    poll(&update);
    update.after_heartbeat(Some(OLD), false);
    assert_eq!(restart.started(), [update.store.path(&commit(newer))]);
    assert_eq!(hub.justiciar_reports().len(), 1);
}

#[test]
fn a_selftest_report_the_hub_did_not_take_is_sent_when_it_answers() {
    let (update, _git, restart, hub, selftest) = daemon_with_selftest();
    selftest.fails.lock().unwrap().push(NEW.to_owned());
    hub.set_unreachable(true);

    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), false);
    assert!(hub.justiciar_reports().is_empty());
    assert!(update.store.is_rejected(&commit(NEW)));

    hub.set_unreachable(false);
    update.after_heartbeat(Some(OLD), false);
    update.after_heartbeat(Some(OLD), false);
    assert_eq!(hub.justiciar_reports().len(), 1);
    assert!(restart.started().is_empty());
}
