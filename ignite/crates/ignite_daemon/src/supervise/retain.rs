//! How long run directories, which hold each command's whole output, are kept.

use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{COLLECTED, OUTPUT, SupervisedLauncher};

/// Seconds a collected run's directory is kept: a day, and 14 days for a run marked with
/// [`keep`] (see "Reporting results" in `docs/ignite.md`).
pub const KEEP_S: u64 = 24 * 60 * 60;
pub const KEEP_JUDGED_S: u64 = 14 * KEEP_S;

/// How often [`SupervisedLauncher::run`] deletes old run directories.
const PRUNE_EVERY_S: u64 = 60 * 60;

/// The extension of the marker that keeps a run's output for [`KEEP_JUDGED_S`].
const KEEP: &str = "keep";

/// Marks the run whose whole output is the file `output` as failed, flaky or incomplete, so
/// [`SupervisedLauncher::prune`] keeps it for [`KEEP_JUDGED_S`]. The marker sits beside the
/// output, so this works for any launcher's output file.
pub fn keep(output: &Path) -> io::Result<()> {
    if output.as_os_str().is_empty() {
        return Ok(());
    }
    File::create(output.with_extension(KEEP)).map(drop)
}

impl SupervisedLauncher {
    /// Deletes the directories of collected runs older than [`KEEP_S`], or [`KEEP_JUDGED_S`] for
    /// a run marked with [`keep`]. A run's age counts from its collection. Returns how many were
    /// deleted. Runs nobody has collected are never deleted.
    pub fn prune(&self, now: SystemTime) -> usize {
        let Ok(entries) = fs::read_dir(&self.runs) else {
            return 0;
        };
        let mut deleted = 0;
        for dir in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let Ok(collected) = fs::metadata(dir.join(COLLECTED)).and_then(|m| m.modified()) else {
                continue;
            };
            let age = now.duration_since(collected).unwrap_or_default().as_secs();
            let limit = if dir.join(OUTPUT).with_extension(KEEP).exists() {
                KEEP_JUDGED_S
            } else {
                KEEP_S
            };
            if age > limit && fs::remove_dir_all(&dir).is_ok() {
                deleted += 1;
            }
        }
        deleted
    }

    /// Runs [`Self::prune`] when it has not run for [`PRUNE_EVERY_S`].
    pub(super) fn prune_now_and_then(&self) {
        let now = SystemTime::now();
        let seconds = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let last = self.pruned.load(Ordering::Relaxed);
        if seconds.saturating_sub(last) >= PRUNE_EVERY_S
            && self
                .pruned
                .compare_exchange(last, seconds, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            self.prune(now);
        }
    }
}
