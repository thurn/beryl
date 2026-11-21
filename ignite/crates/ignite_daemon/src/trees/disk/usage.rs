//! Cached clone-aware usage for doctor and handoffs. A full walk can take minutes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use ignite_os::system::{DiskSpace, disk_space};

use super::Trees;

/// How long a measurement of the trees' disk use is used before it is made again.
const USED_FRESH: Duration = Duration::from_hours(1);

/// The last measurement of a trees folder: when it was made, and the bytes it found.
static USED: Mutex<Option<(PathBuf, Usage)>> = Mutex::new(None);
/// A measurement is running.
static MEASURING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct Usage {
    at: SystemTime,
    tree_bytes: u64,
    volume_bytes: Option<u64>,
}

fn occupied(disk: DiskSpace) -> u64 {
    disk.total.saturating_sub(disk.available)
}

/// The last clone-aware measurement, refreshing in a separate thread when it is stale.
fn last_usage(trees: &Trees) -> Option<Usage> {
    let dir = &trees.config().dir;
    let last = USED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .filter(|(measured, _)| measured == dir)
        .or_else(|| saved(dir));
    let fresh = last
        .as_ref()
        .is_some_and(|(_, usage)| usage.at.elapsed().is_ok_and(|age| age < USED_FRESH));
    if !fresh && !MEASURING.swap(true, Ordering::SeqCst) {
        let trees = trees.clone();
        std::thread::spawn(move || {
            let bytes = trees.build_bytes();
            record(&trees.config().dir, bytes);
            MEASURING.store(false, Ordering::SeqCst);
        });
    }
    last.map(|(_, usage)| usage)
}

pub(crate) fn used_bytes(trees: &Trees) -> Option<u64> {
    last_usage(trees).map(|usage| usage.tree_bytes)
}

/// Charge net growth since the last walk to the trees until a sweep can attribute it.
/// Before the first walk, the space used on the volume is an upper bound.
pub(in crate::trees) fn estimate(trees: &Trees, disk: Option<DiskSpace>) -> Option<u64> {
    estimate_from(last_usage(trees), disk.map(occupied))
}

fn estimate_from(last: Option<Usage>, used_now: Option<u64>) -> Option<u64> {
    let Some(last) = last else {
        return used_now;
    };
    let Some(used_now) = used_now else {
        return Some(last.tree_bytes);
    };
    let growth = used_now.saturating_sub(last.volume_bytes.unwrap_or(used_now));
    Some(last.tree_bytes.saturating_add(growth).min(used_now))
}

/// Reuse a sweep's measurement so diagnostics do not have to walk the trees again.
pub(super) fn record(dir: &Path, bytes: u64) {
    let usage = Usage {
        at: SystemTime::now(),
        tree_bytes: bytes,
        volume_bytes: disk_space(dir).ok().map(occupied),
    };
    let mut last = USED.lock().unwrap_or_else(PoisonError::into_inner);
    save(dir, usage);
    *last = Some((dir.to_path_buf(), usage));
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
fn saved(dir: &Path) -> Option<(PathBuf, Usage)> {
    let text = std::fs::read_to_string(saved_path(dir)).ok()?;
    let mut fields = text.split_whitespace();
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(fields.next()?.parse().ok()?);
    let tree_bytes = fields.next()?.parse().ok()?;
    let volume_bytes = fields.next().and_then(|bytes| bytes.parse().ok());
    Some((
        dir.to_path_buf(),
        Usage {
            at,
            tree_bytes,
            volume_bytes,
        },
    ))
}

fn save(dir: &Path, usage: Usage) {
    let secs = usage
        .at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let bytes = usage.tree_bytes;
    let volume = usage
        .volume_bytes
        .map_or_else(String::new, |n| n.to_string());
    // A measurement that cannot be saved is only measured again by the next daemon.
    let _ = std::fs::write(saved_path(dir), format!("{secs} {bytes} {volume}\n"));
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
        save(
            &dir,
            Usage {
                at,
                tree_bytes: 287,
                volume_bytes: Some(411),
            },
        );
        let (_, read) = saved(&dir).unwrap();
        assert_eq!(read.tree_bytes, 287);
        assert_eq!(read.volume_bytes, Some(411));
        assert!(at.duration_since(read.at).unwrap() < Duration::from_secs(1));
        let _ = std::fs::remove_file(saved_path(&dir));
    }

    #[test]
    fn growth_between_measurements_counts_toward_the_cap() {
        let measured = Usage {
            at: SystemTime::UNIX_EPOCH,
            tree_bytes: 200,
            volume_bytes: Some(500),
        };
        assert_eq!(estimate_from(Some(measured), Some(550)), Some(250));
        assert_eq!(estimate_from(Some(measured), Some(450)), Some(200));
        assert_eq!(estimate_from(None, Some(450)), Some(450));
    }
}
