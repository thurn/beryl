//! How a ready release restarts the daemon: which releases do not restart it, what a restart
//! waits for, and the reason each restart records in the start log.

use ignite_core::fakes::FakeClock;

use super::*;
use crate::update::{Home, start_log, started_at};
use ignite_core::time::Timestamp;

/// A third commit, after `NEW`.
const NEWER: &str = "c7d8e9f";

/// A file of the landing code, which a landing started under an older build would run again
/// under the new one for (see `server/outdated.rs`).
const LANDING_FILE: &str = "ignite/crates/ignite_daemon/src/land/express.rs";

/// A file of the daemon's code outside the landing code.
const DAEMON_FILE: &str = "ignite/crates/ignite_daemon/src/x.rs";

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
fn a_release_that_changes_only_guide_text_is_taken_on_and_the_client_runs_it() {
    let (update, git, restart) = daemon(NEW);
    let home = home_with_this_run(&update);
    git.set_changed(
        &commit(OLD),
        &commit(NEW),
        &["ignite/crates/ignite_core/src/guide/justiciar.txt"],
    );
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.started().is_empty());
    assert_eq!(update.version(), Some(commit(NEW)));
    assert_eq!(this_run(&home).adopted, [NEW]);
    // `ignite guide` is answered by the client from the topics compiled into its executable,
    // and the client runs `bin/current`: that is the release's build, so it prints the new
    // text. The fake builder writes the commit into each build it makes.
    let current = update.store.current_link();
    // On Windows `current_link` is the launcher `bin/current.cmd`, which runs the executable
    // whose path `bin/current` holds.
    let current = if cfg!(windows) {
        std::path::PathBuf::from(
            std::fs::read_to_string(current.with_extension(""))
                .unwrap()
                .trim(),
        )
    } else {
        current
    };
    let client = std::fs::read_to_string(current).unwrap();
    assert_eq!(client, NEW);
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
fn a_release_of_landing_code_waits_for_a_landing_up_to_the_cap() {
    let (update, git, restart, clock) = daemon_with_clock(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &[LANDING_FILE]);
    let home = home_with_this_run(&update);
    update.after_master_moved().unwrap();
    settle(&update);

    update.after_heartbeat(Some(OLD), LANDING);
    clock.advance(WINDOW.cap_seconds - 1.0);
    update.after_heartbeat(Some(OLD), LANDING);
    assert!(restart.started().is_empty());
    clock.advance(1.0);
    update.after_heartbeat(Some(OLD), LANDING);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    let reason = this_run(&home).reason.unwrap();
    assert!(
        reason.ends_with(", after waiting 600 s for landings and newer releases"),
        "{reason}"
    );
}

#[test]
fn a_release_of_landing_code_goes_ahead_once_the_landing_ends() {
    let (update, git, restart, clock) = daemon_with_clock(NEW);
    git.set_changed(&commit(OLD), &commit(NEW), &[LANDING_FILE]);
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
    git.set_changed(&commit(OLD), &commit(NEW), &[LANDING_FILE]);
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

/// A daemon as [`daemon_with_clock`] makes it, with the restart window `ignite.toml` gets by
/// default: a 15 s quiet window and a 10 minute cap.
fn daemon_with_default_window(
    master: &str,
) -> (SelfUpdate, Arc<FakeGit>, FakeRestart, Arc<FakeClock>) {
    let (mut update, git, restart, clock) = daemon_with_clock(master);
    update.window = Mutex::new(RestartWindow::new(
        ignite_core::config::SelfUpdateConfig::default(),
    ));
    (update, git, restart, clock)
}

/// Moves `master` to `release`, which changes `file` of the daemon's code, and builds it.
fn land_release(update: &SelfUpdate, git: &FakeGit, release: &Commit, file: &str) {
    git.set_tip("master", release.clone());
    git.set_changed(&commit(OLD), release, &[file]);
    update.after_master_moved().unwrap();
    settle(update);
}

/// Sends `n` heartbeats 10 s apart, as the sender loop does, with `busy` running.
fn heartbeats(update: &SelfUpdate, clock: &FakeClock, n: usize, busy: Busy) {
    for _ in 0..n {
        update.after_heartbeat(Some(OLD), busy);
        clock.advance(10.0);
    }
}

/// The acceptance test for landings in a row: while landings run one after another, a release
/// that changes daemon code other than the landing code restarts the daemon once no newer
/// release has appeared for the quiet window (15 s), not at the 600 s cap. The landings go on
/// under the build that started them.
#[test]
fn a_release_outside_the_landing_code_restarts_while_landings_run() {
    let (update, git, restart, clock) = daemon_with_default_window(NEW);
    land_release(&update, &git, &commit(NEW), DAEMON_FILE);
    heartbeats(&update, &clock, 2, LANDING);
    assert!(restart.started().is_empty(), "{:?}", restart.started());
    update.after_heartbeat(Some(OLD), LANDING);
    assert_eq!(clock.now(), Timestamp::from_millis(20_000));
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
}

/// Releases come from landings. While the next landing runs, the landing hold merges the
/// releases that change the landing code into one restart; the quiet window then only adds its
/// 15 s.
#[test]
fn three_releases_30_s_apart_while_landings_run_make_one_restart_into_the_last() {
    let (update, git, restart, clock) = daemon_with_default_window(NEW);
    let releases = [commit(NEW), commit(NEWER), commit("f0e1d2c")];
    for release in &releases[..2] {
        land_release(&update, &git, release, LANDING_FILE);
        heartbeats(&update, &clock, 3, LANDING);
        assert!(restart.started().is_empty(), "{:?}", restart.started());
    }
    // The last release appeared at 60 s with no landing left, so the quiet window ends at 75 s.
    land_release(&update, &git, &releases[2], LANDING_FILE);
    heartbeats(&update, &clock, 2, IDLE);
    assert!(restart.started().is_empty(), "{:?}", restart.started());
    assert_eq!(clock.now(), Timestamp::from_millis(80_000));
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&releases[2])]);
}

#[test]
fn the_cap_forces_a_restart_under_a_constant_stream_of_releases() {
    let (update, git, restart, clock) = daemon_with_default_window(NEW);
    let start = clock.now();
    let mut newest = commit(NEW);
    // A release every 30 s, with a landing always running and a heartbeat every 10 s, until
    // the daemon restarts.
    let mut restarted_at = None;
    for round in 0u32.. {
        if round % 3 == 0 {
            newest = commit(&format!("{:07x}", 0x0a0_0000 + round / 3));
            land_release(&update, &git, &newest, LANDING_FILE);
        }
        update.after_heartbeat(Some(OLD), LANDING);
        if !restart.started().is_empty() {
            restarted_at = Some(clock.now());
            break;
        }
        assert!(
            clock.now().seconds_since(start) < 700.0,
            "the restart never came"
        );
        clock.advance(10.0);
    }
    // The heartbeat 10 minutes after the first release restarts the daemon into the newest
    // release built by then.
    assert_eq!(restarted_at, Some(start.plus_seconds(600.0)));
    assert_eq!(restart.started(), [update.store.path(&newest)]);
}

#[test]
fn status_says_when_a_pending_restart_goes_ahead() {
    let (update, git, _restart, clock) = daemon_with_default_window(NEW);
    land_release(&update, &git, &commit(NEW), DAEMON_FILE);
    clock.advance(10.0);
    let Some(Work::Restarting {
        commit: target,
        due,
        cap,
    }) = update.local().work
    else {
        panic!("{:?}", update.local().work);
    };
    assert_eq!(target, commit(NEW));
    assert_eq!(due, Some(Timestamp::from_millis(15_000)));
    assert_eq!(cap, Some(Timestamp::from_millis(600_000)));
}
