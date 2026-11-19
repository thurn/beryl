use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::time::Timestamp;

use super::*;

fn tempdir() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ignite-rollback-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

fn at(seconds: f64) -> Timestamp {
    Timestamp::from_millis(1_800_000_000_000).plus_seconds(seconds)
}

/// A store holding `old`, then `new`, with `current` pointing at `new`.
fn store_with(old: &str, new: &str) -> ExecutableStore {
    let store = ExecutableStore::new(tempdir());
    for hex in [old, new] {
        let built = store.root.join(format!("built-{hex}"));
        fs::write(&built, hex).unwrap();
        store.store(&commit(hex), &built).unwrap();
    }
    store.point_current_at(&commit(old)).unwrap();
    store.point_current_at(&commit(new)).unwrap();
    store
}

#[test]
fn an_update_keeps_the_previous_release() {
    let store = store_with("aaaaaaa", "bbbbbbb");
    assert_eq!(store.previous(), Some(commit("aaaaaaa")));
    #[cfg(unix)]
    assert_eq!(store.current_commit(), Some(commit("bbbbbbb")));
}

#[cfg(unix)]
#[test]
fn a_release_that_starts_twice_without_running_a_minute_is_rolled_back_and_rejected() {
    let (old, new) = (commit("aaaaaaa"), commit("bbbbbbb"));
    let store = store_with("aaaaaaa", "bbbbbbb");
    assert_eq!(store.startup(&new, at(0.0)), Startup::Proceed);
    let Startup::RollBack { to, reason } = store.startup(&new, at(5.0)) else {
        panic!("the second start inside the window rolls back");
    };
    assert_eq!(to, old);
    assert!(reason.contains("bbbbbbb"), "{reason}");
    assert_eq!(store.current_commit(), Some(old.clone()));
    assert!(store.is_rejected(&new));
    let log = fs::read_to_string(store.root.join("rollbacks.log")).unwrap();
    assert!(log.contains("went back to aaaaaaa"), "{log}");
    // The old release is no longer on trial.
    assert_eq!(store.startup(&old, at(6.0)), Startup::Proceed);
    assert_eq!(store.startup(&old, at(7.0)), Startup::Proceed);
}

#[test]
fn a_release_that_ran_long_enough_or_restarts_much_later_stays() {
    let new = commit("bbbbbbb");
    let healthy = store_with("aaaaaaa", "bbbbbbb");
    assert_eq!(healthy.startup(&new, at(0.0)), Startup::Proceed);
    healthy.mark_healthy(&new);
    assert_eq!(healthy.startup(&new, at(10.0)), Startup::Proceed);

    let slow = store_with("aaaaaaa", "bbbbbbb");
    assert_eq!(slow.startup(&new, at(0.0)), Startup::Proceed);
    assert_eq!(
        slow.startup(&new, at(CRASH_WINDOW_S + 1.0)),
        Startup::Proceed
    );
    assert!(!slow.is_rejected(&new));
}

#[cfg(unix)]
#[test]
fn a_release_restarted_on_purpose_stays_and_one_that_then_crashes_is_rolled_back() {
    let new = commit("bbbbbbb");
    let store = store_with("aaaaaaa", "bbbbbbb");
    // An install, then a `kickstart -k` 20 seconds later: the first run stopped cleanly.
    assert_eq!(store.startup(&new, at(0.0)), Startup::Proceed);
    store.record_stop(&new, at(0.0));
    assert_eq!(store.startup(&new, at(20.0)), Startup::Proceed);
    store.record_stop(&new, at(20.0));
    // A stop of a start the trial never saw, or of another release, changes nothing.
    store.record_stop(&new, at(25.0));
    store.record_stop(&commit("aaaaaaa"), at(30.0));
    assert_eq!(store.startup(&new, at(40.0)), Startup::Proceed);
    assert!(!store.is_rejected(&new));
    assert_eq!(store.current_commit(), Some(new.clone()));
    // The run that started at 40 s exits on its own: the next start rolls back.
    let Startup::RollBack { reason, .. } = store.startup(&new, at(50.0)) else {
        panic!("a start after a run with no clean stop rolls back");
    };
    assert!(reason.contains("started 2 times"), "{reason}");
    assert!(store.is_rejected(&new));
}

#[test]
fn a_trial_written_before_stops_were_recorded_still_reads() {
    let new = commit("bbbbbbb");
    let store = store_with("aaaaaaa", "bbbbbbb");
    let seconds = at(0.0).millis() as f64 / 1000.0;
    fs::write(
        store.root.join("trial"),
        format!("{{\"commit\":\"bbbbbbb\",\"starts\":[{seconds}]}}"),
    )
    .unwrap();
    store.record_stop(&new, at(0.0));
    assert_eq!(store.startup(&new, at(5.0)), Startup::Proceed);
}

#[test]
fn pruning_keeps_the_current_previous_and_two_other_releases() {
    let store = store_with("aaaaaaa", "bbbbbbb");
    for (n, hex) in ["ccccccc", "ddddddd", "eeeeeee", "fffffff"]
        .iter()
        .enumerate()
    {
        let dir = store.root.join(hex);
        fs::create_dir_all(&dir).unwrap();
        let file = fs::File::create(dir.join("marker")).unwrap();
        file.set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(n as u64 * 100 + 1),
        )
        .unwrap();
    }
    store.prune(&[&commit("bbbbbbb")]);
    let left = |hex: &str| store.root.join(hex).exists();
    assert!(left("aaaaaaa") && left("bbbbbbb"));
    // Directories' own times are the creation times here, so two of the four others remain.
    let others = ["ccccccc", "ddddddd", "eeeeeee", "fffffff"]
        .iter()
        .filter(|hex| left(hex))
        .count();
    assert_eq!(others, KEEP_EXTRA);
}

#[test]
fn pruning_keeps_a_release_a_live_process_runs() {
    let store = store_with("aaaaaaa", "bbbbbbb");
    for hex in ["ccccccc", "ddddddd", "eeeeeee", "fffffff"] {
        fs::create_dir_all(store.root.join(hex)).unwrap();
    }
    // The oldest other release is still run by a verb process that outlived its daemon.
    let held = super::lock_shared(&store.root.join("ccccccc").join(super::IN_USE)).unwrap();
    // It is also the oldest, so pruning would take it first.
    fs::File::open(store.root.join("ccccccc"))
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1))
        .unwrap();
    store.prune(&[&commit("bbbbbbb")]);
    assert!(store.root.join("ccccccc").exists());
    drop(held);
    store.prune(&[&commit("bbbbbbb")]);
    assert!(!store.root.join("ccccccc").exists());
}

#[test]
fn the_rejection_reason_is_the_newest_log_line_naming_the_commit() {
    let store = store_with("aaaaaaa", "bbbbbbb");
    let bad = commit("bbbbbbb");
    assert_eq!(store.rejection(&bad), None);
    store.reject_with_reason(&bad, "migrations\nfailed", at(0.0));
    store.reject_with_reason(&commit("ccccccc"), "other", at(1.0));
    assert_eq!(
        store.rejection(&bad).as_deref(),
        Some("bbbbbbb failed its selftest: migrations failed")
    );
}
