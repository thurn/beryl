//! How a ready release restarts the daemon: which releases do not restart it, what a restart
//! waits for, and the reason each restart records in the start log.

use ignite_core::fakes::FakeClock;

use super::*;
use crate::update::{Home, start_log, started_at};

/// A third commit, after `NEW`.
const NEWER: &str = "c7d8e9f";

/// A landing runs, and no verb is in a part that may not be cut short.
const LANDING: Busy = Busy {
    critical: false,
    landing: true,
};

/// The home the daemon's executable store is in, with a start record for this run, as the
/// running daemon writes at its start.
fn home_with_this_run(update: &SelfUpdate) -> Home {
    let executable = update.store.path(&commit(OLD));
    let root = executable.ancestors().nth(3).unwrap().to_owned();
    let home = Home { root };
    start_log(&home)
        .record(started_at(), &commit(OLD), std::process::id())
        .unwrap();
    home
}

/// This run's record in the start log of `home`.
fn this_run(home: &Home) -> crate::update::StartRecord {
    start_log(home)
        .read()
        .into_iter()
        .find(|record| record.pid == std::process::id() && record.at == started_at())
        .unwrap()
}

/// A daemon as [`daemon`] makes it, with its clock.
fn daemon_with_clock(master: &str) -> (SelfUpdate, Arc<FakeGit>, FakeRestart, Arc<FakeClock>) {
    let (mut update, git, restart) = daemon(master);
    let clock = Arc::new(FakeClock::new(ignite_core::time::Timestamp::from_millis(0)));
    update.clock = clock.clone();
    (update, git, restart, clock)
}

#[test]
fn a_release_that_changes_none_of_the_daemons_code_is_taken_on_without_a_restart() {
    let (update, git, restart) = daemon(NEW);
    let home = home_with_this_run(&update);
    git.set_changed(
        &commit(OLD),
        &commit(NEW),
        &[
            "ignite/crates/ignite_cli/src/reattach.rs",
            "ignite/crates/ignite_daemon/src/update/tests.rs",
        ],
    );
    update.after_master_moved().unwrap();
    settle(&update);
    // The client gets the release: `bin/current` names it.
    assert_eq!(update.store.current_commit(), Some(commit(NEW)));

    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.started().is_empty());
    assert_eq!(update.version(), Some(commit(NEW)));
    assert_eq!(this_run(&home).adopted, [NEW]);
    assert_eq!(this_run(&home).stopped, None);
    // Once the hub runs the release too, nothing more happens; the release branch at the
    // release is up to date.
    update.after_heartbeat(Some(NEW), IDLE);
    update.after_master_moved().unwrap();
    assert!(update.building().is_none());
    assert!(restart.started().is_empty());
    assert_eq!(update.local().own, Some(commit(NEW)));
}

#[test]
fn a_later_release_that_changes_the_daemon_restarts_it_and_records_why() {
    let (update, git, restart) = daemon(NEW);
    let home = home_with_this_run(&update);
    git.set_changed(
        &commit(OLD),
        &commit(NEW),
        &["ignite/crates/ignite_cli/src/a.rs"],
    );
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.started().is_empty());

    // The next release is compared with the one the daemon took on, not with its own build.
    git.set_tip("master", commit(NEWER));
    git.set_changed(
        &commit(NEW),
        &commit(NEWER),
        &[
            "ignite/crates/ignite_daemon/src/trees.rs",
            "ignite/crates/ignite_daemon/src/trees/tests.rs",
        ],
    );
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEWER))]);
    assert_eq!(
        this_run(&home).reason.as_deref(),
        Some(
            "release c7d8e9f changes the daemon's code: \
             ignite/crates/ignite_daemon/src/trees.rs"
        )
    );
}

#[test]
fn a_hubs_version_that_changes_none_of_the_daemons_code_is_taken_on_too() {
    // Another machine's daemon learns of the release from the hub's heartbeat.
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/AGENTS.md"]);
    update.after_heartbeat(Some(NEW), IDLE);
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
    assert!(restart.started().is_empty());
    assert_eq!(update.version(), Some(commit(NEW)));
}

#[test]
fn a_release_that_does_not_follow_the_daemon_restarts_it_whatever_changed() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &[]);
    git.set_holds(&commit(NEW), &commit(OLD));
    git.set_not_ancestor(&commit(OLD), &commit(NEW));
    update.after_heartbeat(Some(NEW), IDLE);
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_restart_waits_for_a_landing_up_to_the_batching_bound() {
    let (update, _git, restart, clock) = daemon_with_clock(NEW);
    let home = home_with_this_run(&update);
    update.after_master_moved().unwrap();
    settle(&update);

    update.after_heartbeat(Some(OLD), LANDING);
    clock.advance(BATCH_WAIT_S - 1.0);
    update.after_heartbeat(Some(OLD), LANDING);
    assert!(restart.started().is_empty());
    clock.advance(1.0);
    update.after_heartbeat(Some(OLD), LANDING);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    let reason = this_run(&home).reason.unwrap();
    assert!(
        reason.ends_with(", after waiting 120 s for landings and newer releases"),
        "{reason}"
    );
}

#[test]
fn a_restart_goes_ahead_once_the_landing_ends() {
    let (update, _git, restart, clock) = daemon_with_clock(NEW);
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), LANDING);
    clock.advance(10.0);
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn releases_that_land_while_a_restart_waits_make_one_restart_into_the_newest() {
    let (update, git, restart, clock) = daemon_with_clock(NEW);
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), LANDING);

    // Another release lands while the restart waits: it is built, and the restart waits for it
    // even after the landing ended.
    git.set_tip("master", commit(NEWER));
    git.set_changed(
        &commit(OLD),
        &commit(NEWER),
        &["ignite/crates/ignite_daemon/src/x.rs"],
    );
    update.after_master_moved().unwrap();
    clock.advance(10.0);
    update.after_heartbeat(Some(OLD), IDLE);
    settle(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEWER))]);
}

#[test]
fn a_ready_build_waits_only_for_a_push_in_progress() {
    let (update, git, restart) = daemon(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    update.after_master_moved().unwrap();
    settle(&update);

    update.after_heartbeat(Some(OLD), PUSHING);
    assert!(restart.started().is_empty());
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

#[test]
fn a_drill_restart_drains_and_restarts_into_the_given_executable_even_unstored() {
    let (mut update, _git, restart) = daemon(OLD);
    update.own = None;
    let drain = Arc::new(FakeDrain {
        restart: restart.clone(),
        calls: Mutex::default(),
    });
    let _ = update.drain.set(drain.clone());
    let own = PathBuf::from("/opt/ignite/ignite");
    update.restart_now(&own, &|| false);
    assert_eq!(restart.started(), [own]);
    assert_eq!(
        *drain.calls.lock().unwrap(),
        ["drain 30s after 0", "undrain"]
    );
}
