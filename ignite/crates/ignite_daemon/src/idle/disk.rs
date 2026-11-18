//! Holding the trees under `trees_disk_gb`: the idle loop asks [`Trees::hold_disk`] to delete
//! replaced builds, and then the build directories of the free trees used longest ago, whenever
//! the trees are over their limit (`docs/ignite.md`, "Build trees").
//!
//! Measuring the trees walks every file in their build directories, which takes a while on
//! hundreds of gigabytes, so the measurement is made at most once every [`DISK_EVERY`]. Only
//! free trees are ever swept (see `trees/disk.rs`), so a tree that is leased or has a running
//! command is left alone.
//!
//! [`Trees::hold_disk`]: crate::trees::Trees::hold_disk

use std::time::Duration;

use ignite_core::traits::Result;

use super::Idle;
use crate::trees::DiskReport;

/// The least time between two measurements of the trees' disk use. A measurement walks every
/// file of every build directory, about 290 GB on jarvis, so it is kept rare on a busy machine.
pub const DISK_EVERY: Duration = Duration::from_mins(30);

impl Idle {
    /// Brings the trees under their disk limit if they are over it, unless they were measured
    /// less than [`DISK_EVERY`] ago. Returns the report of a measurement made, which is also
    /// printed when it deleted anything.
    pub fn hold_disk(&self) -> Result<Option<DiskReport>> {
        let now = self.clock.now();
        {
            let mut last = super::lock(&self.disk_held);
            if last.is_some_and(|at| now.seconds_since(at) < DISK_EVERY.as_secs_f64()) {
                return Ok(None);
            }
            // The time is taken before the sweep, so a sweep that fails is tried again only
            // after the same wait instead of at every round of the loop.
            *last = Some(now);
        }
        crate::steps::step("holding the trees under their disk limit");
        let report = self.host.hold_disk()?;
        if !report.superseded.is_empty() || !report.cleared.is_empty() {
            let gb = |bytes: u64| bytes as f64 / 1e9;
            eprintln!(
                "ignite: trees held under their disk limit: deleted {} replaced builds and the \
                 build directories of {} free trees ({}), freeing {:.1} GB ({:.1} GB to {:.1} GB \
                 of {})",
                report.superseded.len(),
                report.cleared.len(),
                report
                    .cleared
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                gb(report.before.saturating_sub(report.after)),
                gb(report.before),
                gb(report.after),
                report
                    .limit
                    .map_or_else(|| "no limit".to_owned(), |l| format!("{:.1} GB", gb(l))),
            );
        }
        Ok(Some(report))
    }
}
