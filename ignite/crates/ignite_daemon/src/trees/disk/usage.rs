//! Cached clone-aware usage for doctor. A full walk of the build directories can take minutes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use super::Trees;

/// How long a measurement of the trees' disk use is used before it is made again.
const USED_FRESH: Duration = Duration::from_hours(1);

/// The last measurement of a trees folder: when it was made, and the bytes it found.
static USED: Mutex<Option<(PathBuf, SystemTime, u64)>> = Mutex::new(None);
/// A measurement is running.
static MEASURING: AtomicBool = AtomicBool::new(false);

/// The last clone-aware measurement of the managed build directories. Walking them takes
/// minutes, so start a measurement in a separate thread and return the last value immediately.
pub(crate) fn used_bytes(trees: &Trees) -> Option<u64> {
    let dir = &trees.config().dir;
    let last = USED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .filter(|(measured, _, _)| measured == dir)
        .or_else(|| saved(dir));
    let fresh = last
        .as_ref()
        .is_some_and(|(_, at, _)| at.elapsed().is_ok_and(|age| age < USED_FRESH));
    if !fresh && !MEASURING.swap(true, Ordering::SeqCst) {
        let trees = trees.clone();
        std::thread::spawn(move || {
            let bytes = trees.build_bytes();
            record(&trees.config().dir, bytes);
            MEASURING.store(false, Ordering::SeqCst);
        });
    }
    last.map(|(_, _, bytes)| bytes)
}

/// Reuse a sweep's measurement so diagnostics do not have to walk the trees again.
pub(super) fn record(dir: &Path, bytes: u64) {
    let at = SystemTime::now();
    let mut last = USED.lock().unwrap_or_else(PoisonError::into_inner);
    save(dir, at, bytes);
    *last = Some((dir.to_path_buf(), at, bytes));
}

/// Where the last measurement of `dir` is kept, so that it outlives the daemon: the daemon
/// restarts more often than a measurement of hundreds of gigabytes finishes, and
/// a measurement kept only in memory left `doctor` saying "not measured yet" for good.
fn saved_path(dir: &Path) -> PathBuf {
    let name: String = dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // The daemon's own file, which a later daemon reads back, so not a test's scratch folder.
    #[allow(clippy::disallowed_methods)]
    let temp = std::env::temp_dir();
    temp.join(format!("ignite-trees-build-bytes{name}"))
}

/// The measurement of `dir` a previous daemon saved: seconds since the epoch and bytes.
fn saved(dir: &Path) -> Option<(PathBuf, SystemTime, u64)> {
    let text = std::fs::read_to_string(saved_path(dir)).ok()?;
    let (at, bytes) = text.trim().split_once(' ')?;
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(at.parse().ok()?);
    Some((dir.to_path_buf(), at, bytes.parse().ok()?))
}

fn save(dir: &Path, at: SystemTime, bytes: u64) {
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // A measurement that cannot be saved is only measured again by the next daemon.
    let _ = std::fs::write(saved_path(dir), format!("{secs} {bytes}\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved measurement outlives a daemon restart.
    #[test]
    fn a_measurement_is_read_back_after_a_restart() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-used-test-{}", std::process::id()));
        let at = SystemTime::now();
        save(&dir, at, 287);
        let (_, read_at, bytes) = saved(&dir).unwrap();
        assert_eq!(bytes, 287);
        assert!(at.duration_since(read_at).unwrap() < Duration::from_secs(1));
        let _ = std::fs::remove_file(saved_path(&dir));
    }
}
