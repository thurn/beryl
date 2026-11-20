//! Keeping the previous release and going back to it when a new one does not come up.
//!
//! The login service restarts a daemon that exits, so a release that crashes at startup would
//! crash-loop. Pointing `bin/current` at a release therefore also records:
//!
//! - `bin/previous`: the commit `current` pointed at before, which stays stored.
//! - `bin/trial`: the new release and the times it started, until it has run for
//!   [`HEALTHY_AFTER_S`] ([`ExecutableStore::mark_healthy`]).
//!
//! A release that starts again within [`CRASH_WINDOW_S`] of a start whose run neither became
//! healthy nor stopped cleanly is rolled back ([`ExecutableStore::startup`]): `current` points
//! at the previous release again, the reason goes to `bin/rollbacks.log`, and the commit is added
//! to `bin/rejected` so the daemon does not update to it again.
//!
//! A run stops cleanly when the daemon is asked to stop (`SIGTERM`) or restarts into another
//! build: it then adds its start time to the trial's `stops` ([`ExecutableStore::record_stop`],
//! called from `starts::record_clean_stop`). So a person or a drill that restarts the daemon
//! twice in a minute leaves the release in place, while a release that exits on its own is
//! still rolled back.
//!
//! A process running from a stored release holds a shared lock on the release's [`IN_USE`]
//! file for its whole life ([`hold_release`]), and [`ExecutableStore::prune`] never deletes a
//! release whose lock is held. A verb process outlives the daemon's restarts and points the
//! shared compiler-wrapper and test-runner scripts at its own executable before it runs checks,
//! so deleting its release would make every test those scripts start fail at once.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;
use serde::{Deserialize, Serialize};

use super::ExecutableStore;

/// Seconds a new release must run before it counts as healthy.
pub const HEALTHY_AFTER_S: f64 = 60.0;
/// Two starts of an unhealthy release this close together roll it back.
pub const CRASH_WINDOW_S: f64 = 300.0;
/// Stored releases kept besides the current and previous ones.
pub const KEEP_EXTRA: usize = 2;

/// The file in a release's directory that each process running the release holds a shared
/// lock on.
pub const IN_USE: &str = "in-use";

const PREVIOUS: &str = "previous";
const TRIAL: &str = "trial";
pub(super) const REJECTED: &str = "rejected";
pub(super) const LOG: &str = "rollbacks.log";

#[derive(Debug, Serialize, Deserialize)]
struct Trial {
    commit: String,
    /// Seconds since the Unix epoch of each start of this release.
    starts: Vec<f64>,
    /// The starts, from `starts`, whose runs stopped cleanly because they were asked to.
    #[serde(default)]
    stops: Vec<f64>,
}

/// Seconds since the Unix epoch, as the trial records a start.
fn seconds(at: Timestamp) -> f64 {
    at.millis() as f64 / 1000.0
}

/// Whether two recorded times are the same start.
fn same_start(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.0005
}

/// What a starting daemon should do about its release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Startup {
    Proceed,
    /// Restart into `to`, which `current` already names again.
    RollBack {
        to: Commit,
        reason: String,
    },
}

impl ExecutableStore {
    /// The commit `bin/current` points at.
    pub fn current_commit(&self) -> Option<Commit> {
        #[cfg(unix)]
        let path = self.root.join("current");
        #[cfg(windows)]
        let path =
            std::path::PathBuf::from(fs::read_to_string(self.root.join("current")).ok()?.trim());
        Self::version_of(&path)
    }

    /// The release before the current one, if it is still stored.
    pub fn previous(&self) -> Option<Commit> {
        let commit = self.read_commit(PREVIOUS)?;
        self.has(&commit).then_some(commit)
    }

    /// Whether a rollback rejected `commit`.
    pub fn is_rejected(&self, commit: &Commit) -> bool {
        fs::read_to_string(self.root.join(REJECTED))
            .is_ok_and(|text| text.lines().any(|line| line.trim() == commit.as_str()))
    }

    /// Records that `commit` failed its selftest ([`super::selftest`]) and must not be restarted
    /// into, and why, in `bin/rollbacks.log`. A newer commit is not affected.
    pub fn reject_with_reason(&self, commit: &Commit, reason: &str, now: Timestamp) {
        self.reject(commit);
        self.append(
            LOG,
            &format!(
                "{now} {commit} failed its selftest: {}",
                reason.replace('\n', " ")
            ),
        );
    }

    /// Why `commit` was rejected: the newest line of `bin/rollbacks.log` that names it, without
    /// its time, or `None` when it names it nowhere.
    pub fn rejection(&self, commit: &Commit) -> Option<String> {
        let log = fs::read_to_string(self.root.join(LOG)).ok()?;
        log.lines()
            .rev()
            .find(|line| line.split_whitespace().any(|word| word == commit.as_str()))
            .map(|line| {
                line.split_once(' ')
                    .map_or(line, |(_, rest)| rest)
                    .to_owned()
            })
    }

    pub(super) fn read_commit(&self, name: &str) -> Option<Commit> {
        Commit::new(fs::read_to_string(self.root.join(name)).ok()?.trim()).ok()
    }

    /// Called by [`Self::point_current_at`] before it changes the link: remembers the release
    /// being replaced and starts the trial of the new one.
    pub(super) fn begin_trial(&self, new: &Commit) {
        if let Some(old) = self.current_commit().filter(|old| old != new) {
            let _ = fs::write(self.root.join(PREVIOUS), old.as_str());
        }
        let trial = Trial {
            commit: new.to_string(),
            starts: Vec::new(),
            stops: Vec::new(),
        };
        let _ = self.save_trial(&trial);
    }

    fn save_trial(&self, trial: &Trial) -> io::Result<()> {
        fs::write(self.root.join(TRIAL), serde_json::to_vec(trial)?)
    }

    fn trial(&self) -> Option<Trial> {
        serde_json::from_slice(&fs::read(self.root.join(TRIAL)).ok()?).ok()
    }

    /// Records that release `own` is starting at `now`. If it is a release on trial that
    /// already started within [`CRASH_WINDOW_S`], and that run neither became healthy nor
    /// stopped cleanly, goes back to the previous release (when one is stored).
    pub fn startup(&self, own: &Commit, now: Timestamp) -> Startup {
        let Some(mut trial) = self.trial().filter(|t| t.commit == own.as_str()) else {
            return Startup::Proceed;
        };
        let seconds = seconds(now);
        trial
            .starts
            .retain(|&start| seconds - start < CRASH_WINDOW_S);
        let Trial { starts, stops, .. } = &mut trial;
        stops.retain(|&stop| starts.iter().any(|&start| same_start(start, stop)));
        let before = starts
            .iter()
            .filter(|&&start| !stops.iter().any(|&stop| same_start(start, stop)))
            .count();
        trial.starts.push(seconds);
        let _ = self.save_trial(&trial);
        if before == 0 {
            return Startup::Proceed;
        }
        let Some(to) = self.previous() else {
            return Startup::Proceed;
        };
        let reason = format!(
            "{own} started {} times within {CRASH_WINDOW_S:.0} s without running for \
             {HEALTHY_AFTER_S:.0} s; went back to {to}",
            before + 1
        );
        if self.point_current_at_untried(&to).is_err() {
            return Startup::Proceed;
        }
        let _ = fs::remove_file(self.root.join(TRIAL));
        self.append(REJECTED, own.as_str());
        self.append(LOG, &format!("{now} {reason}"));
        Startup::RollBack { to, reason }
    }

    pub(super) fn append(&self, name: &str, line: &str) {
        use std::io::Write;
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join(name));
        if let Ok(mut file) = file {
            let _ = writeln!(file, "{line}");
        }
    }

    /// Records that the run of release `own` that started at `started` stopped cleanly, so that
    /// [`Self::startup`] does not count it. Does nothing when `own` is not on trial.
    pub fn record_stop(&self, own: &Commit, started: Timestamp) {
        let Some(mut trial) = self.trial().filter(|t| t.commit == own.as_str()) else {
            return;
        };
        let started = seconds(started);
        if trial.starts.iter().any(|&start| same_start(start, started))
            && !trial.stops.iter().any(|&stop| same_start(stop, started))
        {
            trial.stops.push(started);
            let _ = self.save_trial(&trial);
        }
    }

    /// The release on trial has run long enough: ends its trial.
    pub fn mark_healthy(&self, own: &Commit) {
        if self.trial().is_some_and(|t| t.commit == own.as_str()) {
            let _ = fs::remove_file(self.root.join(TRIAL));
        }
    }

    /// Deletes stored releases other than `keep`, the current, previous and last-good ones, those
    /// a live process runs ([`in_use`]) and the [`KEEP_EXTRA`] newest of the rest. A process
    /// whose release was deleted keeps running on macOS, which holds a deleted file open, but
    /// nothing can start that executable again.
    pub fn prune(&self, keep: &[&Commit]) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let mut kept: Vec<String> = keep.iter().map(ToString::to_string).collect();
        kept.extend(self.current_commit().map(|c| c.to_string()));
        kept.extend(self.previous().map(|c| c.to_string()));
        // The build a rollback or the monitor falls back to must survive.
        kept.extend(self.last_good().map(|c| c.to_string()));
        let mut others: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                Commit::new(&name).is_ok()
                    && e.path().is_dir()
                    && !kept.contains(&name)
                    && !in_use(&e.path())
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        others.sort();
        let drop = others.len().saturating_sub(KEEP_EXTRA);
        for (_, path) in others.into_iter().take(drop) {
            let _ = fs::remove_dir_all(path);
        }
    }

    /// Flips `current` without starting a trial, as a rollback does.
    fn point_current_at_untried(&self, commit: &Commit) -> io::Result<()> {
        self.flip(commit)
    }

    /// Writes `file` and flushes it to disk, then renames it into place and flushes the
    /// directory, so a crash after the rename cannot leave an empty executable.
    pub(super) fn sync_rename(partial: &Path, dest: &Path) -> io::Result<()> {
        // Windows flushes a file only through a handle that may write it.
        fs::OpenOptions::new()
            .write(true)
            .open(partial)?
            .sync_all()?;
        fs::rename(partial, dest)?;
        #[cfg(unix)]
        if let Some(dir) = dest.parent() {
            fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    }
}

/// Takes a shared lock on the [`IN_USE`] file of the stored release that `executable` was
/// started from, and keeps it until the process ends, so that [`ExecutableStore::prune`] leaves
/// the release in place. Does nothing for an executable outside the store. The lock's file is
/// closed when the process replaces its image with another release, so a restarted daemon
/// holds only its new release.
pub fn hold_release(executable: &Path) {
    static HELD: OnceLock<Option<fs::File>> = OnceLock::new();
    HELD.get_or_init(|| {
        ExecutableStore::version_of(executable)?;
        let dir = ignite_os::paths::canonicalize(executable)
            .ok()?
            .parent()?
            .to_path_buf();
        let file = lock_shared(&dir.join(IN_USE));
        if let Err(error) = &file {
            eprintln!(
                "ignite: a newer release may delete {} while it runs: {error}",
                dir.display()
            );
        }
        file.ok()
    });
}

/// Opens `path`, creating it, and takes a shared lock on it.
pub(super) fn lock_shared(path: &Path) -> io::Result<fs::File> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    file.lock_shared()?;
    Ok(file)
}

/// Whether a live process holds the lock on the [`IN_USE`] file of the release in `dir`. A
/// release with no such file, from before releases were locked, counts as unused.
fn in_use(dir: &Path) -> bool {
    let Ok(file) = fs::OpenOptions::new().write(true).open(dir.join(IN_USE)) else {
        return false;
    };
    // A lock held elsewhere, or one that cannot be taken at all: keep the release rather than
    // risk deleting a running one.
    file.try_lock().is_err()
}

#[cfg(test)]
#[path = "rollback_tests.rs"]
mod tests;
