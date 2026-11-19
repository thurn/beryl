//! The memory commands usually need, so the scheduler can hold back a command that needs more
//! memory than is free (see "Cores, memory and shared resources" in `docs/ignite.md`).
//!
//! The launcher keeps the peak memory of each command line's last few runs in `peaks.json` in
//! its runs folder, and asks the scheduler for the median of them.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use super::write_json;

/// Runs of one command line whose peak memory is kept.
const KEPT: usize = 9;
/// Command lines remembered; when more are recorded, the alphabetically first is forgotten.
const COMMANDS: usize = 500;

#[derive(Debug)]
pub(super) struct Peaks {
    path: PathBuf,
    by_command: Mutex<BTreeMap<String, Vec<u64>>>,
}

/// What identifies a command: its program and arguments.
pub(super) fn key(args: &[String]) -> String {
    args.join("\u{1f}")
}

impl Peaks {
    /// The peaks stored at `path`, or none if the file is missing or unreadable.
    pub(super) fn load(path: PathBuf) -> Self {
        let by_command = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            by_command: Mutex::new(by_command),
        }
    }

    /// The median peak memory, in megabytes, of the command's recent runs.
    pub(super) fn usual(&self, key: &str) -> Option<u64> {
        let by_command = self
            .by_command
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        median(by_command.get(key)?)
    }

    /// Whether the command's last `runs` or more recorded runs all measured under `limit`.
    pub(super) fn all_below(&self, key: &str, limit: u64, runs: usize) -> bool {
        let by_command = self
            .by_command
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        by_command
            .get(key)
            .is_some_and(|kept| kept.len() >= runs && kept.iter().all(|&value| value < limit))
    }

    /// Remembers a finished run's peak memory. A run that measured nothing is not recorded.
    pub(super) fn record(&self, key: &str, peak_mb: u64) {
        if peak_mb == 0 {
            return;
        }
        let mut by_command = self
            .by_command
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let peaks = by_command.entry(key.to_owned()).or_default();
        peaks.push(peak_mb);
        if peaks.len() > KEPT {
            peaks.remove(0);
        }
        while by_command.len() > COMMANDS {
            by_command.pop_first();
        }
        // A file that cannot be written only loses the history at the next restart.
        let _ = write_json(&self.path, &*by_command);
    }
}

fn median(peaks: &[u64]) -> Option<u64> {
    let mut sorted = peaks.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usual_peak_is_the_median_of_the_last_runs_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("ignite-peaks-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("peaks.json");
        let peaks = Peaks::load(path.clone());
        let command = key(&["cargo".to_owned(), "build".to_owned()]);
        assert_eq!(peaks.usual(&command), None);
        peaks.record(&command, 0);
        assert_eq!(
            peaks.usual(&command),
            None,
            "nothing measured, nothing kept"
        );
        for mb in [100, 900, 300] {
            peaks.record(&command, mb);
        }
        assert_eq!(peaks.usual(&command), Some(300));
        for _ in 0..KEPT {
            peaks.record(&command, 50);
        }
        assert_eq!(peaks.usual(&command), Some(50), "old runs age out");
        assert_eq!(Peaks::load(path).usual(&command), Some(50));
        fs::remove_dir_all(&dir).unwrap();
    }
}
