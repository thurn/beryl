//! The disk loop asks [`Trees::hold_disk`] to clear builds above the current budget. It
//! clears builds without measuring first while the volume has less than [`FLOOR_GB`] free
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

use ignite_core::capacity::{BYTES_PER_GB, BYTES_PER_GIB, disk_reserve_bytes};
use ignite_core::traits::Result;
use ignite_os::system::DiskSpace;

use super::Idle;
use crate::trees::DiskReport;

/// The least time between two measurements of the trees' disk use. A measurement walks every
/// file of every build directory, about 290 GB on jarvis, so it is kept rare on a busy machine.
pub const DISK_EVERY: Duration = Duration::from_mins(30);

/// Skip the expensive walk when at least this much disk is free and the volume reserve
/// and cap already fit.
pub const ROOM_GB: u64 = 150;

/// Below this much free space, clear builds without walking every tree first.
pub use ignite_core::dispatch_pressure::DISK_FLOOR_GB as FLOOR_GB;

/// The least time between two clearing passes while the disk is below [`FLOOR_GB`].
pub const LOW_EVERY: Duration = Duration::from_mins(5);

/// Free space already covers the reserve, and the cap could hold every used block even
/// if all were in the trees.
fn has_disk_room(disk: DiskSpace, cap_gb: Option<u32>) -> bool {
    disk.available >= (ROOM_GB * BYTES_PER_GIB).max(disk_reserve_bytes(disk.total))
        && cap_gb.is_none_or(|cap| {
            disk.total.saturating_sub(disk.available) <= u64::from(cap) * BYTES_PER_GB
        })
}

impl Idle {
    /// Brings the trees under their disk limit if they are over it, unless they were measured
    /// less than [`DISK_EVERY`] ago. Returns the report of a measurement made, which is also
    /// printed when it deleted anything.
    pub fn hold_disk(&self) -> Result<Option<DiskReport>> {
        let disk = self.host.disk_space();
        if disk.is_some_and(|disk| has_disk_room(disk, self.host.disk_cap_gb())) {
            return Ok(None);
        }
        let below_floor = disk.is_some_and(|disk| disk.available < FLOOR_GB * BYTES_PER_GIB);
        let every = if below_floor { LOW_EVERY } else { DISK_EVERY };
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
        let report = if below_floor {
            crate::steps::step("clearing free trees until the disk is back above its floor");
            self.host.free_disk(FLOOR_GB)?
        } else {
            crate::steps::step("holding the trees under their disk limit");
            self.host.hold_disk()?
        };
        let gb = |bytes: u64| bytes as f64 / BYTES_PER_GB as f64;
        let limit = match (report.limit, report.limit_source) {
            (Some(limit), Some(source)) => format!("{:.1} GB (set by the {source})", gb(limit)),
            _ => "unavailable".to_owned(),
        };
        if !report.superseded.is_empty()
            || !report.cleared.is_empty()
            || !report.incremental.is_empty()
            || !report.reclaimed.is_empty()
            || report.objects > 0
        {
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
                 ({}), freeing {:.1} GB more ({:.1} GB to {:.1} GB, limit {limit})",
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
            );
        } else if !below_floor {
            eprintln!(
                "ignite: tree disk budget {limit}, {:.1} GB in use after sweep",
                gb(report.after)
            );
        }
        Ok(Some(report))
    }
}
