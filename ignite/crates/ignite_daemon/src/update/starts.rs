//! The start log: one line in `starts.jsonl` for each time a daemon started, which is how a
//! crash loop is seen. The log is written at the very top of `ignite daemon`, before anything
//! of the new build's own code can fail.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use ignite_core::ids::Commit;
use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;
use serde::{Deserialize, Serialize};

use super::lastgood::{LAST_GOOD_AFTER_S, detect_crash_loop, roll_back};
use super::{ExecRestart, ExecutableStore, HEALTHY_AFTER_S, Home, Restart, restart_into};

/// The log keeps this many of the newest records.
const KEEP: usize = 50;

/// One start of a daemon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRecord {
    pub at: Timestamp,
    pub commit: String,
    pub pid: u32,
}

/// The file `<home>/starts.jsonl`.
#[derive(Clone, Debug)]
pub struct StartLog {
    path: PathBuf,
}

/// The start log in `home`.
pub fn start_log(home: &Home) -> StartLog {
    StartLog {
        path: home.root.join("starts.jsonl"),
    }
}

impl StartLog {
    /// The records in the file, oldest first. A line that is not a record is skipped.
    pub fn read(&self) -> Vec<StartRecord> {
        let text = fs::read_to_string(&self.path).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Appends a record and trims the file to the newest [`KEEP`] records.
    pub fn record(&self, now: Timestamp, commit: &Commit, pid: u32) -> io::Result<()> {
        let mut records = self.read();
        records.push(StartRecord {
            at: now,
            commit: commit.to_string(),
            pid,
        });
        self.write(&records)
    }

    /// Removes the records process `pid` wrote.
    pub fn discard(&self, pid: u32) -> io::Result<()> {
        let mut records = self.read();
        records.retain(|record| record.pid != pid);
        self.write(&records)
    }

    fn write(&self, records: &[StartRecord]) -> io::Result<()> {
        let skip = records.len().saturating_sub(KEEP);
        let mut text = String::new();
        for record in &records[skip..] {
            text.push_str(&serde_json::to_string(record)?);
            text.push('\n');
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let partial = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        fs::File::create(&partial)?.write_all(text.as_bytes())?;
        fs::rename(&partial, &self.path)
    }
}

/// The commit of the executable that is running, or `None` for one that is not in the store.
fn own_commit() -> Option<Commit> {
    ExecutableStore::version_of(&std::env::current_exe().ok()?)
}

/// The first thing a starting daemon does: records this start, and when this build has
/// crash-looped and a last-good build exists, goes back to that build instead. Returns the exit
/// code if the daemon should stop here, which is only when the restart into the last-good build
/// failed.
pub fn guard_start(home: &Home) -> Option<i32> {
    let own = own_commit()?;
    guard(
        home,
        &own,
        SystemClock.now(),
        std::process::id(),
        &ExecRestart,
    )
}

fn guard(
    home: &Home,
    own: &Commit,
    now: Timestamp,
    pid: u32,
    restart: &dyn Restart,
) -> Option<i32> {
    let log = start_log(home);
    // A log that cannot be written must not keep the daemon from starting.
    let _ = log.record(now, own, pid);
    let store = home.store();
    let looped = detect_crash_loop(&log.read(), own, store.last_good(), now)?;
    if let Err(error) = roll_back(home, &looped, now) {
        eprintln!("ignite: cannot go back to {}: {error}", looped.to);
        return None;
    }
    eprintln!(
        "ignite: {} started {} times in a few minutes; going back to {}",
        looped.bad, looped.starts, looped.to
    );
    let error = restart_into(restart, &store.path(&looped.to));
    // Only a failed restart returns.
    eprintln!("ignite: {error}");
    Some(1)
}

/// Removes this process's start record, for a daemon that exits because another one holds the
/// machine's lock: a second launch of a healthy daemon is no crash.
pub fn forget_start(home: &Home) {
    let _ = start_log(home).discard(std::process::id());
}

/// Starts the thread that ends the trial of `own` after [`HEALTHY_AFTER_S`] and marks it the
/// last good build after [`LAST_GOOD_AFTER_S`], if the process is still up.
pub fn spawn_health_marks(store: ExecutableStore, own: Commit) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(HEALTHY_AFTER_S));
        store.mark_healthy(&own);
        std::thread::sleep(Duration::from_secs_f64(LAST_GOOD_AFTER_S - HEALTHY_AFTER_S));
        if !store.is_rejected(&own) {
            let _ = store.mark_last_good(&own);
        }
    });
}

#[cfg(test)]
#[path = "starts_tests.rs"]
mod tests;
