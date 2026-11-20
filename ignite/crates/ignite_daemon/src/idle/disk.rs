//! Holding the trees under `trees_disk_gb`: the disk loop asks [`Trees::hold_disk`] to delete
//! replaced builds, and then the build directories of the free trees used longest ago, whenever
//! the trees are over their limit or the disk has less than [`FLOOR_GB`] free
//! (`docs/ignite.md`, "Build trees").
//!
//! Measuring the trees walks every file in their build directories, which takes a while on
//! hundreds of gigabytes, so the measurement is made at most once every [`DISK_EVERY`], or
//! every [`LOW_EVERY`] while the disk is below its floor. Free trees are swept first; a leased
//! tree is swept only while nothing runs in it, and a tree running a command is left alone (see
//! `trees/disk.rs`).
//!
//! This ran in the idle loop until a round of idle work waited 107 minutes for cores while the
//! disk filled from 130 GB free to 1.6 GB, with no measurement made. It runs in a loop of its
//! own now, the disk loop in `main/loops.rs`.
//!
//! [`Trees::hold_disk`]: crate::trees::Trees::hold_disk

use std::time::Duration;

use ignite_core::traits::Result;

use super::Idle;
use crate::trees::DiskReport;

/// The least time between two measurements of the trees' disk use. A measurement walks every
/// file of every build directory, about 290 GB on jarvis, so it is kept rare on a busy machine.
pub const DISK_EVERY: Duration = Duration::from_mins(30);

/// Free disk space at which the trees are not measured at all. A measurement walked about
/// 290 GB of build directories for over ten minutes on jarvis, holding the idle loop in one
/// round, while 255 GB of the disk was free; with that much room nothing needs deleting.
pub const ROOM_GB: u64 = 150;

/// Free disk space the daemon keeps. Below it the trees are held under their current size
/// less what the disk lacks, whatever `trees_disk_gb` allows: that limit was set for the disk
/// as it was then, and the rest of the disk can fill after it was set.
pub const FLOOR_GB: u64 = 50;

/// The least time between two measurements while the disk is below [`FLOOR_GB`].
pub const LOW_EVERY: Duration = Duration::from_mins(5);

impl Idle {
    /// Brings the trees under their disk limit if they are over it, unless they were measured
    /// less than [`DISK_EVERY`] ago. Returns the report of a measurement made, which is also
    /// printed when it deleted anything.
    pub fn hold_disk(&self) -> Result<Option<DiskReport>> {
        let free = self.host.disk_free_gb();
        if free.is_some_and(|free| free >= ROOM_GB) {
            return Ok(None);
        }
        let short_gb = free.map_or(0, |free| FLOOR_GB.saturating_sub(free));
        let every = if short_gb > 0 { LOW_EVERY } else { DISK_EVERY };
        let now = self.clock.now();
        {
            let mut last = super::lock(&self.disk_held);
            if last.is_some_and(|at| now.seconds_since(at) < every.as_secs_f64()) {
                return Ok(None);
            }
            // The time is taken before the sweep, so a sweep that fails is tried again only
            // after the same wait instead of at every round of the loop.
            *last = Some(now);
        }
        // Below the floor, free trees are cleared without measuring the trees first: two full
        // measurements of 1.6 TB took over an hour, the daemon restarted for a release before
        // any round finished, and the disk stayed full with every build waiting for space.
        let report = if short_gb > 0 {
            crate::steps::step("clearing free trees until the disk is back above its floor");
            self.host.free_disk(FLOOR_GB)?
        } else {
            crate::steps::step("holding the trees under their disk limit");
            self.host.hold_disk(0)?
        };
        if !report.superseded.is_empty()
            || !report.cleared.is_empty()
            || !report.incremental.is_empty()
            || !report.reclaimed.is_empty()
            || report.objects > 0
        {
            let gb = |bytes: u64| bytes as f64 / 1e9;
            let names = |trees: &[ignite_core::ids::TreeId]| {
                trees
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            eprintln!(
                "ignite: trees held under their disk limit: deleted {} stale object files \
                 ({:.1} GB), {} replaced builds, the incremental caches of {} idle leased trees \
                 ({}), and the build directories of {} free trees ({}) and {} idle leased trees \
                 ({}), freeing {:.1} GB more ({:.1} GB to {:.1} GB \
                 of {})",
                report.objects,
                gb(report.object_bytes),
                report.superseded.len(),
                report.incremental.len(),
                names(&report.incremental),
                report.cleared.len(),
                names(&report.cleared),
                report.reclaimed.len(),
                names(&report.reclaimed),
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
