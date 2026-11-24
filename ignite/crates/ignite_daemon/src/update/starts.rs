//! The start log: one line in `starts.jsonl` for each time a daemon started, which is how a
//! crash loop is seen. The log is written at the very top of `ignite daemon`, before anything
//! of the new build's own code can fail.
//!
//! A daemon that is asked to stop records a clean stop on its own line before it exits
//! ([`record_clean_stop`]): on `SIGTERM` (`launchctl kickstart -k`, `bootout`, an install) and
//! before it restarts into another build. The record goes into `bin/trial` too, and neither
//! crash-loop guard counts a run that stopped cleanly, so restarting the daemon on purpose never
//! rolls its build back. A run that ended with no record (a crash, `SIGKILL`, a machine that lost
//! power, or on Windows a stop of the scheduled task, which ends the process outright) still
//! counts. So does a run the monitor kickstarted because it stopped answering: the monitor marks
//! it hung first ([`StartLog::mark_hung`]), and a hung run records no clean stop.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
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
    /// When this run stopped because it was asked to; `None` for a run that is still up or that
    /// ended any other way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Timestamp>,
    /// Whether the monitor kickstarted this run because it did not answer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hung: bool,
    /// Why this run stopped on purpose, such as the release it restarted into and the files of
    /// the daemon's code that release changed, so that a postmortem can count the restarts a
    /// release caused and the ones that waited for a landing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Releases this run took on without restarting, because they changed nothing the daemon
    /// runs (see `SelfUpdate::after_heartbeat`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adopted: Vec<String>,
}

impl StartRecord {
    /// Whether this start counts towards a crash loop: its run did not stop cleanly.
    pub fn counts(&self) -> bool {
        self.stopped.is_none()
    }
}

/// When this process started, as recorded in the start log.
static RUN_START: OnceLock<Timestamp> = OnceLock::new();

/// The time this process recorded its start at, which names its run in the start log and in
/// `bin/trial`.
pub fn started_at() -> Timestamp {
    *RUN_START.get_or_init(|| SystemClock.now())
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
            stopped: None,
            hung: false,
            reason: None,
            adopted: Vec::new(),
        });
        self.write(&records)
    }

    /// Records that the run of process `pid` that started at `started` stopped cleanly at
    /// `now`, for `reason`. Returns `false`, and records nothing, when the monitor marked that
    /// run hung.
    pub fn mark_stopped(
        &self,
        pid: u32,
        started: Timestamp,
        now: Timestamp,
        reason: &str,
    ) -> io::Result<bool> {
        let mut records = self.read();
        let Some(record) = records
            .iter_mut()
            .rev()
            .find(|record| record.pid == pid && record.at == started)
        else {
            return Ok(true);
        };
        if record.hung {
            return Ok(false);
        }
        record.stopped = Some(now);
        record.reason = Some(reason.to_owned());
        self.write(&records)?;
        Ok(true)
    }

    /// Records that the run of process `pid` that started at `started` took on the release
    /// `commit` without restarting. A run whose start was not logged records nothing.
    pub fn mark_adopted(&self, pid: u32, started: Timestamp, commit: &Commit) -> io::Result<()> {
        let mut records = self.read();
        let Some(record) = records
            .iter_mut()
            .rev()
            .find(|record| record.pid == pid && record.at == started)
        else {
            return Ok(());
        };
        record.adopted.push(commit.to_string());
        self.write(&records)
    }

    /// Marks the newest run as hung, before the monitor kickstarts a daemon that does not
    /// answer: the `SIGTERM` of the kickstart then records no clean stop, and the run counts.
    pub fn mark_hung(&self) -> io::Result<()> {
        let mut records = self.read();
        let Some(record) = records.last_mut() else {
            return Ok(());
        };
        record.hung = true;
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
    let now = started_at();
    let own = own_commit()?;
    guard(home, &own, now, std::process::id(), &ExecRestart)
}

/// Records that this daemon stopped because it was asked to, and `reason`, in the start log and
/// in `bin/trial`, so that neither crash-loop guard counts this run. Called on `SIGTERM` and
/// just before the daemon restarts into another build.
pub fn record_clean_stop(home: &Home, reason: &str) {
    clean_stop(
        home,
        own_commit().as_ref(),
        started_at(),
        std::process::id(),
        SystemClock.now(),
        reason,
    );
}

/// The home that holds `store`.
pub(crate) fn home_of(store: &ExecutableStore) -> Option<Home> {
    store.root.parent().map(|root| Home {
        root: root.to_owned(),
    })
}

/// [`record_clean_stop`] for the home that holds `store`, for a caller that has only the store:
/// the daemon's restart into a new release.
pub fn record_clean_stop_from(store: &ExecutableStore, reason: &str) {
    if let Some(home) = home_of(store) {
        record_clean_stop(&home, reason);
    }
}

/// Records in the start log of the home that holds `store` that this run took on the release
/// `commit` without restarting.
pub fn record_adopted_from(store: &ExecutableStore, commit: &Commit) {
    if let Some(home) = home_of(store) {
        let _ = start_log(&home).mark_adopted(std::process::id(), started_at(), commit);
    }
}

/// Records the clean stop in the start log, and in `bin/trial` when the running build is
/// `own`, a stored one.
fn clean_stop(
    home: &Home,
    own: Option<&Commit>,
    started: Timestamp,
    pid: u32,
    now: Timestamp,
    reason: &str,
) {
    // A run whose start was not logged still records its stop in the trial.
    let clean = start_log(home)
        .mark_stopped(pid, started, now, reason)
        .unwrap_or(true);
    if clean && let Some(own) = own {
        home.store().record_stop(own, started);
    }
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
        // A machine installed before the monitor existed, or whose monitor still runs an
        // older build, gets it (re)installed here, pointed at the build just marked good.
        let monitor = ignite_os::service::Service::install_monitor(
            &ignite_os::Native,
            &store.monitor_program(),
            &[crate::update::MONITOR_VERB.to_owned()],
        );
        if let Err(error) = monitor
            && error.kind() != std::io::ErrorKind::Unsupported
        {
            eprintln!("ignite: the monitor was not installed: {error}");
        }
    });
}

#[cfg(test)]
#[path = "starts_tests.rs"]
mod tests;
