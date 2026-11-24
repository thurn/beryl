use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;

use super::*;
use crate::update::Startup;
use crate::update::lastgood::{CRASH_STARTS, CrashLoop, detect_crash_loop, roll_back};

const GOOD: &str = "aaaaaaa";
const BAD: &str = "bbbbbbb";

fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

fn at(seconds: f64) -> Timestamp {
    Timestamp::from_millis(1_800_000_000_000).plus_seconds(seconds)
}

/// A home with `GOOD` and `BAD` stored, `current` on `BAD` and `GOOD` marked last-good.
fn home() -> Home {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let root = ignite_core::test_files::temp_dir().join(format!(
        "ignite-starts-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = Home { root };
    let store = home.store();
    for hex in [GOOD, BAD] {
        std::fs::create_dir_all(&store.root).unwrap();
        let built = store.root.join(format!("built-{hex}"));
        std::fs::write(&built, hex).unwrap();
        store.store(&commit(hex), &built).unwrap();
    }
    store.point_current_at(&commit(BAD)).unwrap();
    store.mark_last_good(&commit(GOOD)).unwrap();
    home
}

fn records(commit_hex: &str, seconds: &[f64]) -> Vec<StartRecord> {
    seconds
        .iter()
        .map(|&s| StartRecord {
            at: at(s),
            commit: commit_hex.to_owned(),
            pid: 1,
            stopped: None,
            hung: false,
            reason: None,
            adopted: Vec::new(),
        })
        .collect()
}

#[derive(Default)]
struct FakeRestart {
    started: RefCell<Vec<PathBuf>>,
}

impl Restart for FakeRestart {
    fn restart(&self, executable: &Path, _args: &[String]) -> std::io::Result<()> {
        self.started.borrow_mut().push(executable.to_owned());
        Ok(())
    }
}

#[test]
fn the_log_keeps_only_the_newest_fifty_records() {
    let home = home();
    let log = start_log(&home);
    for i in 0..60 {
        log.record(at(f64::from(i)), &commit(BAD), 100 + i as u32)
            .unwrap();
    }
    let records = log.read();
    assert_eq!(records.len(), 50);
    assert_eq!(records[0].pid, 110);
    assert_eq!(records[49].pid, 159);
    // One JSON object per line.
    let text = std::fs::read_to_string(home.root.join("starts.jsonl")).unwrap();
    assert_eq!(text.lines().count(), 50);
    assert!(
        text.lines()
            .all(|l| l.starts_with('{') && l.contains("\"commit\""))
    );
}

#[test]
fn a_damaged_line_is_skipped_and_a_launch_that_lost_the_lock_is_forgotten() {
    let home = home();
    let log = start_log(&home);
    log.record(at(0.0), &commit(BAD), 7).unwrap();
    let path = home.root.join("starts.jsonl");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("not json\n");
    std::fs::write(&path, text).unwrap();
    log.record(at(1.0), &commit(BAD), 8).unwrap();
    assert_eq!(log.read().len(), 2);
    log.discard(8).unwrap();
    assert_eq!(log.read().iter().map(|r| r.pid).collect::<Vec<_>>(), [7]);
}

#[test]
fn three_starts_in_five_minutes_are_a_crash_loop_when_a_last_good_build_exists() {
    let starts = records(BAD, &[0.0, 60.0, 120.0]);
    let looped = detect_crash_loop(&starts, &commit(BAD), Some(commit(GOOD)), at(130.0));
    assert_eq!(
        looped,
        Some(CrashLoop {
            bad: commit(BAD),
            to: commit(GOOD),
            starts: CRASH_STARTS
        })
    );
    // Two starts, starts of another build and starts outside the window do not count.
    assert!(detect_crash_loop(&starts[..2], &commit(BAD), Some(commit(GOOD)), at(130.0)).is_none());
    let other = records(GOOD, &[0.0, 60.0, 120.0]);
    assert!(detect_crash_loop(&other, &commit(BAD), Some(commit(GOOD)), at(130.0)).is_none());
    assert!(detect_crash_loop(&starts, &commit(BAD), Some(commit(GOOD)), at(500.0)).is_none());
    // Nothing to go back to, or the last-good build is the one that crash-loops.
    assert!(detect_crash_loop(&starts, &commit(BAD), None, at(130.0)).is_none());
    assert!(detect_crash_loop(&starts, &commit(BAD), Some(commit(BAD)), at(130.0)).is_none());
}

#[test]
fn the_third_start_rolls_back_and_the_bad_build_is_never_installed_again() {
    let home = home();
    let restart = FakeRestart::default();
    for (i, expect_stop) in [(0.0, false), (30.0, false), (60.0, true)] {
        let stopped = guard(&home, &commit(BAD), at(i), 10 + i as u32, &restart);
        assert_eq!(stopped.is_some(), expect_stop, "start at {i}");
    }
    let store = home.store();
    assert_eq!(store.current_commit(), Some(commit(GOOD)));
    assert_eq!(*restart.started.borrow(), [store.path(&commit(GOOD))]);
    assert!(store.is_rejected(&commit(BAD)));
    assert!(!store.is_rejected(&commit("ccccccc")));

    let crash = std::fs::read_dir(home.root.join("crash")).unwrap();
    let files: Vec<_> = crash.map(|e| e.unwrap().path()).collect();
    assert_eq!(files.len(), 1);
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(record["commit"], BAD);
    assert_eq!(record["thread"], "startup");
    assert_eq!(
        record["message"],
        format!("crash loop: 3 starts in 5 minutes; rolled back to {GOOD}")
    );
    assert_eq!(record["location"], "");
    assert_eq!(record["backtrace"], "");
    assert!(record["at"].as_str().is_some());
}

/// Clean stops in quick succession do not count towards a crash loop.
#[test]
fn starts_after_clean_stops_are_not_a_crash_loop() {
    let home = home();
    let restart = FakeRestart::default();
    for (i, pid) in [(0.0, 10), (20.0, 11), (40.0, 12), (60.0, 13)] {
        assert_eq!(guard(&home, &commit(BAD), at(i), pid, &restart), None);
        assert_eq!(home.store().startup(&commit(BAD), at(i)), Startup::Proceed);
        clean_stop(
            &home,
            Some(&commit(BAD)),
            at(i),
            pid,
            at(i + 10.0),
            "a release",
        );
    }
    let store = home.store();
    assert!(restart.started.borrow().is_empty());
    assert_eq!(store.current_commit(), Some(commit(BAD)));
    assert!(!store.is_rejected(&commit(BAD)));
    let log = start_log(&home).read();
    assert_eq!(log[0].stopped, Some(at(10.0)));
    assert!(log.iter().all(|record| !record.counts()));
}

/// A build that exits on its own, with no clean stop recorded, is still rolled back, also when
/// clean restarts came before.
#[test]
fn starts_after_crashes_still_roll_back_among_clean_stops() {
    let home = home();
    let restart = FakeRestart::default();
    // A clean stop, then two crashes: the third start that did not follow a clean stop rolls
    // back.
    assert_eq!(guard(&home, &commit(BAD), at(0.0), 10, &restart), None);
    clean_stop(&home, Some(&commit(BAD)), at(0.0), 10, at(5.0), "a release");
    assert_eq!(guard(&home, &commit(BAD), at(10.0), 11, &restart), None);
    assert_eq!(guard(&home, &commit(BAD), at(20.0), 12, &restart), None);
    assert_eq!(guard(&home, &commit(BAD), at(30.0), 13, &restart), Some(1));
    let store = home.store();
    assert_eq!(store.current_commit(), Some(commit(GOOD)));
    assert!(store.is_rejected(&commit(BAD)));
}

#[test]
fn a_clean_stop_marks_only_its_own_run_and_old_lines_still_read() {
    let home = home();
    let log = start_log(&home);
    // A line written before records carried a stop.
    std::fs::write(
        home.root.join("starts.jsonl"),
        format!(
            "{{\"at\":\"{}\",\"commit\":\"{BAD}\",\"pid\":7}}\n",
            at(0.0)
        ),
    )
    .unwrap();
    // The same process ID after an exec into another build is another run.
    log.record(at(30.0), &commit(BAD), 7).unwrap();
    assert!(
        log.mark_stopped(
            7,
            at(30.0),
            at(40.0),
            "release e4f5a6b changes the daemon's code"
        )
        .unwrap()
    );
    let records = log.read();
    assert_eq!(records.len(), 2);
    assert!(records[0].counts());
    assert_eq!(records[1].stopped, Some(at(40.0)));
    assert_eq!(
        records[1].reason.as_deref(),
        Some("release e4f5a6b changes the daemon's code")
    );
    let text = std::fs::read_to_string(home.root.join("starts.jsonl")).unwrap();
    assert!(!text.lines().next().unwrap().contains("stopped"));
    assert!(!text.contains("hung"));
}

#[test]
fn a_release_taken_on_without_a_restart_is_listed_on_its_run() {
    let home = home();
    let log = start_log(&home);
    log.record(at(0.0), &commit(GOOD), 7).unwrap();
    log.mark_adopted(7, at(0.0), &commit(BAD)).unwrap();
    // Another run's adoption leaves this one alone.
    log.mark_adopted(8, at(0.0), &commit(GOOD)).unwrap();
    assert_eq!(log.read()[0].adopted, [BAD]);
}

#[test]
fn the_last_good_build_survives_pruning_and_a_missing_one_is_not_marked() {
    let home = home();
    let store = home.store();
    // Many newer builds, none of which is current or last-good.
    for i in 0..6 {
        let hex = format!("{i}00000{i}");
        let built = store.root.join(format!("built-{hex}"));
        std::fs::write(&built, &hex).unwrap();
        store.store(&commit(&hex), &built).unwrap();
    }
    store.prune(&[]);
    assert!(store.has(&commit(GOOD)), "the last-good build is kept");
    assert!(store.has(&commit(BAD)), "the current build is kept");
    assert!(store.mark_last_good(&commit("ddddddd")).is_err());
    assert!(store.point_current_at(&commit("ddddddd")).is_err());
    assert_eq!(store.current_commit(), Some(commit(BAD)));
}

#[test]
fn the_monitor_runs_the_last_good_build_when_there_is_one() {
    let home = home();
    let store = home.store();
    assert_eq!(store.monitor_program(), store.last_good_link());
    #[cfg(windows)]
    {
        assert_eq!(
            store.monitor_program().extension().and_then(|e| e.to_str()),
            Some("cmd")
        );
        // A home whose `bin/last-good` predates the `.cmd` launcher gets one on demand.
        std::fs::remove_file(store.last_good_link()).unwrap();
        let program = store.monitor_program();
        assert_eq!(program, store.last_good_link());
        assert_eq!(
            std::fs::read_to_string(&program).unwrap(),
            crate::update::lastgood::LAST_GOOD_CMD
        );
    }
    let bare = ExecutableStore::new(home.root.join("elsewhere"));
    assert_eq!(bare.monitor_program(), bare.current_link());
}

#[test]
fn rolling_back_without_a_stored_target_fails_and_changes_nothing() {
    let home = home();
    let looped = CrashLoop {
        bad: commit(BAD),
        to: commit("eeeeeee"),
        starts: 3,
    };
    assert!(roll_back(&home, &looped, at(0.0)).is_err());
    assert!(!home.store().is_rejected(&commit(BAD)));
    assert_eq!(home.store().current_commit(), Some(commit(BAD)));
}
