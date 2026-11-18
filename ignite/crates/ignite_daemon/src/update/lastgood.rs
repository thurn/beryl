//! The last build that is known to work, and the decision to fall back to it.
//!
//! A daemon that has stayed up for [`LAST_GOOD_AFTER_S`] marks its own build with `bin/last-good`
//! ([`ExecutableStore::mark_last_good`]), a link of the same kind as `bin/current`. Every start
//! of a daemon is appended to `starts.jsonl` (see `starts`). A build that starts
//! [`CRASH_STARTS`] times within [`CRASH_WINDOW_S`], not counting starts whose run stopped
//! cleanly because it was asked to, is in a crash loop: the login service keeps
//! restarting it, so it is replaced by the last-good build ([`roll_back`]), recorded as bad so
//! that no update installs that exact commit again, and described in a record in `crash/`.
//! Both the starting daemon and the monitor make this decision, so it works whichever of them
//! notices the loop first.

use std::fs;
use std::io;
use std::path::PathBuf;

use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;

use super::rollback::{LOG, REJECTED};
use super::starts::StartRecord;
use super::{CRASH_WINDOW_S, ExecutableStore, Home};

/// Seconds a daemon must stay up before its build counts as the last good one.
pub const LAST_GOOD_AFTER_S: f64 = 600.0;
/// This many starts of one build within [`CRASH_WINDOW_S`] are a crash loop.
pub const CRASH_STARTS: usize = 3;

const LAST_GOOD: &str = "last-good";

impl ExecutableStore {
    /// `bin/last-good`, which points at the last build that stayed up.
    pub fn last_good_link(&self) -> PathBuf {
        self.root.join(LAST_GOOD)
    }

    /// The last-good build, if one is marked and still stored.
    pub fn last_good(&self) -> Option<Commit> {
        #[cfg(unix)]
        let path = self.last_good_link();
        #[cfg(windows)]
        let path = PathBuf::from(fs::read_to_string(self.last_good_link()).ok()?.trim());
        Self::version_of(&path).filter(|commit| self.has(commit))
    }

    /// Marks `commit`, which must be stored, as the last good build.
    pub fn mark_last_good(&self, commit: &Commit) -> io::Result<()> {
        if !self.has(commit) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the build of {commit} is not stored"),
            ));
        }
        self.link(LAST_GOOD, commit)
    }

    /// What the monitor's login service entry runs: the last-good build if there is one, else
    /// the current one, so the monitor itself runs a build known to work.
    pub fn monitor_program(&self) -> PathBuf {
        if self.last_good().is_some() {
            self.last_good_link()
        } else {
            self.current_link()
        }
    }

    /// Records that `commit` crash-looped, so that no update installs it again. A newer commit
    /// is not affected.
    pub fn reject(&self, commit: &Commit) {
        let _ = fs::create_dir_all(&self.root);
        self.append(REJECTED, commit.as_str());
    }
}

/// A build that keeps crashing at startup, and the build to go back to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashLoop {
    pub bad: Commit,
    pub to: Commit,
    /// How many starts of `bad` that did not stop cleanly fall in the window.
    pub starts: usize,
}

/// Whether `bad`, the build the login service starts, crash-looped at `now` according to the
/// recorded `starts` whose runs did not stop cleanly, and the last-good build to go back to. There is nothing to go back to
/// when no build is marked or the marked one is `bad` itself.
pub fn detect_crash_loop(
    starts: &[StartRecord],
    bad: &Commit,
    last_good: Option<Commit>,
    now: Timestamp,
) -> Option<CrashLoop> {
    let to = last_good.filter(|good| good != bad)?;
    let count = starts
        .iter()
        .filter(|start| start.commit == bad.as_str() && start.counts())
        .filter(|start| {
            let age = (now.millis() - start.at.millis()) as f64 / 1000.0;
            (0.0..CRASH_WINDOW_S).contains(&age)
        })
        .count();
    (count >= CRASH_STARTS).then(|| CrashLoop {
        bad: bad.clone(),
        to,
        starts: count,
    })
}

/// Points `bin/current` at the build to go back to, records the bad build and writes the
/// crash record. The caller restarts the daemon.
pub fn roll_back(home: &Home, looped: &CrashLoop, now: Timestamp) -> io::Result<()> {
    let store = home.store();
    if !store.has(&looped.to) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("the build of {} is not stored", looped.to),
        ));
    }
    store.flip(&looped.to)?;
    store.reject(&looped.bad);
    let message = format!(
        "crash loop: {} starts in {} minutes; rolled back to {}",
        looped.starts,
        (CRASH_WINDOW_S / 60.0) as u32,
        looped.to
    );
    store.append(LOG, &format!("{now} {} {message}", looped.bad));
    let dir = home.root.join("crash");
    fs::create_dir_all(&dir)?;
    let record = serde_json::json!({
        "at": now,
        "commit": looped.bad,
        "thread": "startup",
        "message": message,
        "location": "",
        "backtrace": "",
    });
    fs::write(
        dir.join(format!("{}-startup.json", now.millis())),
        serde_json::to_vec_pretty(&record)?,
    )
}
