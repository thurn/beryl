//! The disk loop asks [`Trees::hold_disk`] to clear builds above the current budget, and
//! [`Trees::free_disk`] to clear builds without measuring first whenever the volume has less than
//! [`FLOOR_GB`] free (`docs/ignite.md`, "Build trees").
//!
//! Measuring the trees walks every file in their build directories, which takes a while on
//! hundreds of gigabytes, so the measurement is made at most once every [`DISK_EVERY`]. Free
//! trees are swept first; a leased tree is swept only while nothing runs in it, and a tree
//! running a command is left alone (see `trees/disk.rs`).
//!
//! This ran in the idle loop until a round of idle work waited 107 minutes for cores while the
//! disk filled from 130 GB free to 1.6 GB, with no measurement made. It runs in a loop of its
//! own now, the disk loop in `main/loops.rs`.
//!
//! The pass below the floor comes before everything else the disk loop does. A measurement can
//! take many minutes, and the disk could fall below the floor during one and wait for it to end,
//! so the walk reads the disk's free space before each directory and stops once it is below the
//! floor, and the floor pass runs at once. The loop also runs the floor pass before each of its
//! other steps. When a floor pass finds nothing more it may delete, it says what holds the space
//! (`trees/disk/floor.rs`), and the next round tries again rather than waiting minutes.
//!
//! [`Trees::hold_disk`]: crate::trees::Trees::hold_disk
//! [`Trees::free_disk`]: crate::trees::Trees::free_disk

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

/// Free space already covers the reserve, and the cap could hold every used block even
/// if all were in the trees.
fn has_disk_room(disk: DiskSpace, cap_gb: Option<u32>) -> bool {
    disk.available >= (ROOM_GB * BYTES_PER_GIB).max(disk_reserve_bytes(disk.total))
        && cap_gb.is_none_or(|cap| {
            disk.total.saturating_sub(disk.available) <= u64::from(cap) * BYTES_PER_GB
        })
}

impl Idle {
    /// Whether the disk holding the trees has less than [`FLOOR_GB`] free. A disk whose free
    /// space cannot be read is not.
    pub fn below_floor(&self) -> bool {
        self.host
            .disk_space()
            .is_some_and(|disk| disk.available < FLOOR_GB * BYTES_PER_GIB)
    }

    /// Clears builds without measuring the trees while the disk is below [`FLOOR_GB`], every
    /// time it is called. `None` when the disk is above the floor. A report whose
    /// [`DiskReport::stuck`] is set says what holds the space; the disk loop reports it as a
    /// failed round and calls this again next round.
    ///
    /// The disk filled to 4 GB free while two full measurements of 1.6 TB took over an hour, and
    /// the daemon restarted for a release before any round finished. This pass reads only the
    /// disk's free space, so it can run each round.
    pub fn hold_floor(&self) -> Result<Option<DiskReport>> {
        if !self.below_floor() {
            *super::lock(&self.floor_stuck) = None;
            return Ok(None);
        }
        crate::steps::step("clearing builds until the disk is back above its floor");
        let report = self.host.free_disk(FLOOR_GB)?;
        log(&report, true);
        if let Some(stuck) = &report.stuck {
            let mut last = super::lock(&self.floor_stuck);
            // Said once while it stays the same, since the loop's failed round shows it too.
            if last.as_ref() != Some(stuck) {
                eprintln!("ignite: {stuck}");
            }
            *last = Some(stuck.clone());
        } else {
            *super::lock(&self.floor_stuck) = None;
        }
        Ok(Some(report))
    }

    /// Reclaims unused outputs if the trees are over their budget, unless they were measured
    /// less than [`DISK_EVERY`] ago. Returns the report of a measurement made, which is also
    /// printed when it deleted anything. Below the floor this is [`Idle::hold_floor`]. If the
    /// disk falls below the floor during the measurement, the measurement stops, the floor pass
    /// runs at once and its report is returned, and the next round measures again.
    pub fn hold_disk(&self) -> Result<Option<DiskReport>> {
        if self.below_floor() {
            return self.hold_floor();
        }
        let disk = self.host.disk_space();
        if disk.is_some_and(|disk| has_disk_room(disk, self.host.disk_cap_gb())) {
            return Ok(None);
        }
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
        let report = self.host.hold_disk(&|| self.below_floor())?;
        if report.interrupted {
            eprintln!(
                "ignite: the disk fell below its {FLOOR_GB} GiB floor while the trees were \
                 measured; the measurement stopped so builds could be cleared now"
            );
            log(&report, true);
            // Measured again next round instead of a full period later, since this one did
            // not finish.
            *super::lock(&self.disk_held) = None;
            return Ok(self.hold_floor()?.or(Some(report)));
        }
        log(&report, false);
        Ok(Some(report))
    }
}

/// Prints what a sweep deleted, or, for a full measurement that deleted nothing, the budget and
/// the bytes in use. `quiet` leaves out the second.
fn log(report: &DiskReport, quiet: bool) {
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
            "ignite: tree disk sweep deleted {} stale object files \
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
    } else if !quiet {
        eprintln!(
            "ignite: tree disk budget {limit}, {:.1} GB in use after sweep",
            gb(report.after)
        );
    }
}
