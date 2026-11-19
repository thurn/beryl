use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex as StdMutex};
use std::time::{Duration, Instant};

use ignite_core::fakes::{FakeClock, FakeGit, FakeHub};
use ignite_core::ids::MachineId;

use super::*;
use crate::deploy::{Checked, DeployState, Work};

const OLD: &str = "a1b2c3d";
const NEW: &str = "e4f5a6b";

/// The restart window of most tests: no quiet window, so a ready release restarts the daemon at
/// the next heartbeat unless something holds it, and the default cap.
const WINDOW: ignite_core::config::SelfUpdateConfig = ignite_core::config::SelfUpdateConfig {
    quiet_seconds: 0.0,
    cap_seconds: 600.0,
};

/// Nothing runs that a restart waits for.
const IDLE: Busy = Busy {
    critical: false,
    landing: false,
};
/// A landing is pushing.
const PUSHING: Busy = Busy {
    critical: true,
    landing: true,
};

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

/// A daemon running `OLD`, with `master` at `master`. `NEW` changes the daemon's code.
fn daemon(master: &str) -> (SelfUpdate, Arc<FakeGit>, FakeRestart) {
    let dir = tempdir();
    let git = Arc::new(FakeGit::default());
    git.set_tip("master", commit(master));
    git.set_changed(
        &commit(OLD),
        &commit(NEW),
        &["ignite/crates/ignite_daemon/src/x.rs"],
    );
    let restart = FakeRestart::default();
    let update = SelfUpdate {
        own: Some(commit(OLD)),
        git: git.clone(),
        builder: Arc::new(FakeBuilder { out: dir.clone() }),
        store: ExecutableStore::new(dir.join("bin")),
        master: "master".to_owned(),
        checks: "manifests".to_owned(),
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
        hub_service: Arc::new(FakeHubService::default()),
        hub_kicked: Mutex::default(),
        drain: std::sync::OnceLock::new(),
        adopted: Mutex::default(),
        release_seen: Mutex::default(),
        window: Mutex::new(RestartWindow::new(WINDOW)),
    };
    (update, git, restart)
}

/// The hub's login service on the hub's machine. Each kickstart records how many restarts of
/// the daemon had happened before it, so a test can tell that the hub went first.
#[derive(Default)]
struct FakeHubService {
    installed: bool,
    restart: FakeRestart,
    kicks: Mutex<Vec<usize>>,
}

impl HubService for FakeHubService {
    fn installed(&self) -> bool {
        self.installed
    }

    fn kickstart(&self) -> io::Result<()> {
        self.kicks
            .lock()
            .unwrap()
            .push(self.restart.started().len());
        Ok(())
    }
}

impl FakeHubService {
    fn kicks(&self) -> Vec<usize> {
        self.kicks.lock().unwrap().clone()
    }
}

/// A daemon on the hub's machine, as [`daemon`] makes it, with its hub's login service and its
/// clock.
fn hub_machine_daemon(
    master: &str,
) -> (
    SelfUpdate,
    Arc<FakeGit>,
    FakeRestart,
    Arc<FakeHubService>,
    Arc<FakeClock>,
) {
    let (mut update, git, restart) = daemon(master);
    let service = Arc::new(FakeHubService {
        installed: true,
        restart: restart.clone(),
        kicks: Mutex::default(),
    });
    let clock = Arc::new(FakeClock::new(ignite_core::time::Timestamp::from_millis(0)));
    update.hub_service = service.clone();
    update.clock = clock.clone();
    (update, git, restart, service, clock)
}

#[test]
fn a_daemon_on_another_version_than_the_hub_restarts_into_the_hubs_at_once() {
    let (update, _git, restart) = daemon(NEW);

    // Commands may run; the restart does not wait for them. The hub's version is built on a
    // thread of its own, and the heartbeat after it ends restarts into it.
    update.after_heartbeat(Some(NEW), IDLE);
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_daemon_on_the_hubs_version_stays() {
    let (update, _git, restart) = daemon(OLD);
    update.after_heartbeat(Some(OLD), IDLE);
    // A hub that has not answered yet changes nothing.
    update.after_heartbeat(None, IDLE);
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
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn on_the_hub_machine_the_hub_restarts_before_the_daemon() {
    let (update, git, restart, service, _clock) = hub_machine_daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    update.after_master_moved().unwrap();
    settle(&update);
    // `bin/current` points at the new build before either restarts, so the hub starts it.
    assert_eq!(update.store.current_commit(), Some(commit(NEW)));

    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    // One kickstart of the hub, before any restart of the daemon.
    assert_eq!(service.kicks(), [0]);
}

#[test]
fn a_hub_already_on_the_release_is_not_restarted_again() {
    // The hub reports `NEW` and this daemon follows it; the hub needs no restart.
    let (update, _git, restart, service, _clock) = hub_machine_daemon(NEW);
    update.after_heartbeat(Some(NEW), IDLE);
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    assert!(service.kicks().is_empty());
}

#[test]
fn a_hub_behind_the_daemon_on_its_machine_is_restarted_not_followed() {
    // This daemon runs `bin/current`, `OLD`; the hub still reports `NEW`, an older or other
    // build (its restart failed, or its service was installed after the release).
    let (update, _git, restart, service, clock) = hub_machine_daemon(NEW);
    let built = tempdir().join("built-old");
    std::fs::write(&built, OLD).unwrap();
    update.store.store(&commit(OLD), &built).unwrap();
    update.store.point_current_at(&commit(OLD)).unwrap();

    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(service.kicks(), [0]);
    // Not again on every heartbeat, but again after a while.
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(service.kicks().len(), 1);
    clock.advance(HUB_KICK_GAP_S + 1.0);
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(service.kicks().len(), 2);
    // The daemon never went to the hub's build.
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
    assert!(restart.started().is_empty());
}

#[test]
fn a_hub_left_on_a_rejected_release_is_moved_back_with_the_daemon() {
    // The daemon crash-looped on `NEW` and was rolled back to `OLD`, which `bin/current` now
    // names; the hub still serves `NEW`.
    let (update, _git, _restart, service, _clock) = hub_machine_daemon(NEW);
    let built = tempdir().join("built-old");
    std::fs::write(&built, OLD).unwrap();
    update.store.store(&commit(OLD), &built).unwrap();
    update.store.point_current_at(&commit(OLD)).unwrap();
    update
        .store
        .reject_with_reason(&commit(NEW), "crash loop", update.clock.now());

    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(service.kicks(), [0]);
}

#[test]
fn master_moving_without_ignite_changes_keeps_the_executable() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["docs/readme.md"]);

    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.started().is_empty());
    assert!(!update.store.has(&commit(NEW)));
}

#[test]
fn a_daemon_already_on_masters_tip_waits_for_the_hub_to_catch_up() {
    // This daemon runs `OLD`, which is `master`'s tip; the hub still reports an older commit.
    let (update, _git, restart) = daemon(OLD);
    update.after_heartbeat(Some(NEW), IDLE);
    assert!(restart.started().is_empty());
}

#[test]
fn an_unstored_daemon_never_updates() {
    let (mut update, _git, restart) = daemon(NEW);
    update.own = None;
    update.after_heartbeat(Some(NEW), IDLE);
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
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

/// A landing on this machine moves the release branch from a worktree of the checkout, so the
/// next round of the sender loop sees it without a fetch and builds it, instead of waiting for
/// the regular check every `POLL_EVERY_ROUNDS` rounds.
#[test]
fn a_release_moved_in_the_checkout_builds_at_the_next_round() {
    let (mut update, git, restart) = daemon(OLD);
    update.master = "release".to_owned();
    git.set_tip("release", commit(OLD));
    update.rounds.store(1, Ordering::Relaxed);
    update.poll_release().unwrap();
    settle(&update);

    git.set_tip("release", commit(NEW));
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    update.poll_release().unwrap();
    settle(&update);
    assert!(update.store.has(&commit(NEW)));
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_release_branch_that_has_not_moved_builds_nothing() {
    let (update, _git, restart) = daemon(OLD);
    poll(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.started().is_empty());
    assert!(!update.store.has(&commit(NEW)));
}

#[test]
fn a_release_move_without_ignite_changes_builds_nothing() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["docs/readme.md"]);
    poll(&update);
    update.after_heartbeat(Some(OLD), IDLE);
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
    update.after_heartbeat(Some(NEW), IDLE);
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
    update.after_heartbeat(Some(OLD), IDLE);
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
        update.after_heartbeat(Some(NEW), IDLE);
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
    assert_eq!(
        update.failed().as_ref().map(|(failed, _)| failed),
        Some(&commit(NEW))
    );

    // Neither the sender's poll, the tip loop nor the hub's version tries the same commit again.
    poll(&update);
    update.after_master_moved().unwrap();
    update.after_heartbeat(Some(NEW), IDLE);
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
    update.after_heartbeat(Some(NEW), IDLE);
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
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
        update.after_heartbeat(Some(NEW), IDLE);
        update.after_heartbeat(Some(OLD), IDLE);
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
    update.after_heartbeat(Some(OLD), IDLE);
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
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(hub.justiciar_reports().is_empty());
    assert!(update.store.is_rejected(&commit(NEW)));

    hub.set_unreachable(false);
    update.after_heartbeat(Some(OLD), IDLE);
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(hub.justiciar_reports().len(), 1);
    assert!(restart.started().is_empty());
}

/// Records each drain with how many restarts had happened before it, and each undrain.
#[derive(Default)]
struct FakeDrain {
    restart: FakeRestart,
    calls: Mutex<Vec<String>>,
}

impl crate::server::Drain for FakeDrain {
    fn drain(&self, within: std::time::Duration) {
        let restarts = self.restart.started().len();
        self.calls
            .lock()
            .unwrap()
            .push(format!("drain {}s after {restarts}", within.as_secs()));
    }

    fn undrain(&self) {
        self.calls.lock().unwrap().push("undrain".to_owned());
    }
}

#[test]
fn a_restart_stops_taking_requests_and_waits_for_the_daemons_own_verbs_first() {
    let (update, _git, restart) = daemon(NEW);
    let drain = Arc::new(FakeDrain {
        restart: restart.clone(),
        calls: Mutex::default(),
    });
    let _ = update.drain.set(drain.clone());
    update.after_heartbeat(Some(NEW), IDLE);
    settle(&update);
    assert!(
        drain.calls.lock().unwrap().is_empty(),
        "nothing to restart into yet"
    );
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    // The fake restart returns, as a failed one does, so the server takes requests again.
    assert_eq!(
        *drain.calls.lock().unwrap(),
        ["drain 30s after 0", "undrain"]
    );
}

/// A selftest that waits for its gate, so a test can see the daemon while it runs.
struct GatedSelftest(Gate);

impl Selftest for GatedSelftest {
    fn run(&self, _executable: &Path) -> Result<String> {
        let (lock, wake) = &*self.0;
        let mut open = lock.lock().unwrap();
        while !*open {
            open = wake.wait(open).unwrap();
        }
        Ok("selftest passed".to_owned())
    }
}

/// `ignite status` and `ignite wait deploy-<commit>` see the build, its selftest and the ready
/// build the next heartbeat restarts into.
#[test]
fn the_deploy_state_follows_the_build_its_selftest_and_the_restart() {
    let (mut update, git, _restart) = daemon(NEW);
    let gate: Gate = Arc::new((StdMutex::new(false), Condvar::new()));
    update.selftest = Arc::new(GatedSelftest(gate.clone()));
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/a.rs"]);
    assert_eq!(update.check().unwrap(), (commit(NEW), Checked::Build));
    assert_eq!(update.local().work, None);

    update.poll_release().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while update.local().work != Some(Work::SelfTesting(commit(NEW))) {
        assert!(Instant::now() < deadline, "{:?}", update.local().work);
        thread::sleep(Duration::from_millis(2));
    }
    open_gate(&gate);
    settle(&update);
    assert!(
        matches!(&update.local().work, Some(Work::Restarting { commit: c, .. }) if *c == commit(NEW)),
        "{:?}",
        update.local().work
    );
}

/// A deploy wait makes the next round of the sender loop check the release branch, instead of
/// waiting up to five minutes for the regular check.
#[test]
fn a_deploy_wait_makes_the_next_round_check_the_release_branch() {
    let (update, git, _restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/a.rs"]);
    let now = update.clock.now();
    // The regular check has just run.
    update.rounds.store(1, Ordering::Relaxed);
    assert_eq!(
        update.local().next_check,
        Some(now.plus_seconds(f64::from(POLL_EVERY_ROUNDS - 1) * 10.0))
    );
    update.check_soon();
    assert_eq!(update.local().next_check, Some(now));
    update.poll_release().unwrap();
    settle(&update);
    assert!(update.store.has(&commit(NEW)));
}

#[test]
fn the_deploy_state_gives_why_a_build_failed_or_was_rolled_back() {
    let (builder, gate, _started) = SlowBuilder::new(tempdir(), true);
    let (update, git, _restart) = daemon_building_with(builder);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/a.rs"]);
    open_gate(&gate);
    poll(&update);
    assert_eq!(
        update.local().work,
        Some(Work::Failed(
            commit(NEW),
            format!("cargo build failed for {NEW}")
        ))
    );

    assert_eq!(update.rejection(&commit(NEW)), None);
    update
        .store
        .reject_with_reason(&commit(NEW), "bad manifest", update.clock.now());
    assert_eq!(
        update.rejection(&commit(NEW)).as_deref(),
        Some(format!("{NEW} failed its selftest: bad manifest").as_str())
    );
    assert_eq!(update.check().unwrap(), (commit(NEW), Checked::Rejected));

    // A commit that contains the release has reached it; one the release is not in has not.
    assert!(update.reached(&commit(NEW), &commit(OLD)));
    git.set_not_ancestor(&commit(NEW), &commit(OLD));
    assert!(!update.reached(&commit(OLD), &commit(NEW)));
    // A commit that holds the release's changes as a rebased copy has reached it too.
    git.set_holds(&commit(OLD), &commit(NEW));
    assert!(update.reached(&commit(OLD), &commit(NEW)));
}

#[path = "selfupdate_restart_tests.rs"]
mod restart;
