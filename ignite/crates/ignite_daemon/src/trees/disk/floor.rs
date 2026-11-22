//! Getting the disk back above its floor of free space, and saying what holds the space when
//! that cannot be done safely.
//!
//! [`Trees::free_disk`] deletes the build directories of free trees and of leased trees that run
//! nothing. A tree running a command keeps its builds, since deleting files a build is reading or
//! writing breaks that build. So the disk can stay below its floor with nothing left that the
//! daemon may delete: every tree holding builds runs a command or waits for one, or the space is
//! held outside the trees altogether. On 2026-11-18 files outside the trees grew until the disk
//! was full, and on 2026-11-20 every tree was leased and busy. Both times the log said only what
//! was deleted, so nobody could tell why the disk stayed full.
//!
//! Now such a pass returns a [`FloorStuck`] that names each tree still holding builds and what
//! runs in it, or says that the trees hold nothing more to delete. The disk loop reports it as
//! the reason its round failed, which `ignite status` and `ignite doctor` show, and runs the pass
//! again every round, so the first tree whose command ends is cleared within a minute. New builds
//! meanwhile wait for disk space before they start (`build/room.rs`), and new handoffs wait for
//! room above the floor (`ignite_core::dispatch_pressure`).

use std::fmt;

use ignite_core::capacity::BYTES_PER_GIB;
use ignite_core::ids::TreeId;
use ignite_core::traits::Result;
use ignite_store::local::trees as store;
use serde::Serialize;

use super::super::{Trees, store_error};
use super::{DiskLimitSource, DiskReport, id_of, objects};

/// What kept [`Trees::free_disk`] from getting the disk back above its floor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FloorStuck {
    /// Bytes free on the disk when the pass ended, and the floor it was to reach.
    pub free: u64,
    pub floor: u64,
    /// The trees that still hold build directories, each with what keeps it from being cleared.
    /// Empty when no tree holds builds the pass could have deleted: the space is held outside
    /// the trees' build directories, or by a snapshot that keeps deleted blocks allocated.
    pub busy: Vec<BusyTree>,
}

/// A tree whose build directories the floor pass had to keep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BusyTree {
    pub tree: TreeId,
    pub why: String,
}

impl fmt::Display for FloorStuck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let gib = |bytes: u64| bytes as f64 / BYTES_PER_GIB as f64;
        write!(
            f,
            "the disk has {:.1} GiB free, under its {:.0} GiB floor, and nothing more can be \
             deleted safely: ",
            gib(self.free),
            gib(self.floor)
        )?;
        if self.busy.is_empty() {
            return f.write_str(
                "no tree holds builds to delete, so the space is held outside the trees' build \
                 directories or by a snapshot; free space outside the trees",
            );
        }
        let trees: Vec<String> = self
            .busy
            .iter()
            .map(|busy| format!("{} ({})", busy.tree, busy.why))
            .collect();
        write!(
            f,
            "every tree holding builds is in use: {}. The disk loop clears each one as soon as \
             nothing runs in it; to free space sooner, stop one of these commands with `ignite \
             stop` or free space outside the trees",
            trees.join(", ")
        )
    }
}

impl Trees {
    /// Frees disk space fast while the disk has less than `floor` bytes free, without
    /// measuring the trees first. Deletes the whole build directories of free trees, used
    /// longest ago first, and reads the disk's free space with `free` after each one, stopping
    /// once it reaches `floor` or cannot be read. With no free tree left, it deletes the stale
    /// object files and incremental caches of the leased trees that run nothing, and then their
    /// whole build directories, leased longest ago first (`disk/leased.rs`). If the disk is still
    /// below `floor` after all that, [`DiskReport::stuck`] says what holds the space.
    ///
    /// [`Trees::hold_disk`] measures every tree in full twice before it clears any, which took
    /// over half an hour per measurement on 1.6 TB of build directories under load. The daemon
    /// restarted for each release every few minutes, so no round ever finished, and the disk
    /// filled to 4 GB free with every build waiting for space. A full disk needs space now; which
    /// tree holds the most bytes of its own matters less than freeing any. A tree whose files
    /// are clones of another's frees little, and the next free tree goes after it.
    pub fn free_disk(&self, floor: u64, free: impl Fn() -> Option<u64>) -> Result<DiskReport> {
        // A stopped verb may have been ended before it could release its landing tree.
        // Reclaim that tree before the sweep, so the next round can build there.
        self.free_dead_owners(&self.lock())?;
        let mut report = DiskReport {
            limit: Some(floor),
            limit_source: Some(DiskLimitSource::FreeSpaceFloor),
            ..DiskReport::default()
        };
        // Once the disk is back above the floor, or its free space cannot be read, nothing
        // more is deleted and the free space is not read again.
        let done = std::cell::Cell::new(false);
        let last = std::cell::Cell::new(None);
        let low = || {
            let read = if done.get() { None } else { free() };
            last.set(read.or(last.get()));
            let low = read.is_some_and(|bytes| bytes < floor);
            done.set(!low);
            low
        };
        for row in self.free_rows()? {
            if !low() {
                break;
            }
            let id = id_of(&row)?;
            if self.clear_free(&id)? {
                report.cleared.push(id);
            }
        }
        if low() {
            let mut objects = objects::Swept::default();
            self.each_idle_leased(|dir| objects += objects::sweep_objects(dir))?;
            report.objects = objects.files;
            report.object_bytes = objects.bytes;
        }
        if low() {
            report.incremental = self.drop_incremental()?;
        }
        for row in self.leased_rows()? {
            if !low() {
                break;
            }
            let id = id_of(&row)?;
            if self.clear_idle_leased(&id)? {
                report.reclaimed.push(id);
            }
        }
        if low() {
            report.stuck = Some(FloorStuck {
                free: last.get().unwrap_or_default(),
                floor,
                busy: self.busy_trees()?,
            });
        }
        self.serve_waiters();
        Ok(report)
    }

    /// The trees that still hold any of their build directories, each with what keeps it from
    /// being cleared.
    fn busy_trees(&self) -> Result<Vec<BusyTree>> {
        let config = &self.inner.config;
        let keep = config.keep.get();
        let rows = store::all(&self.lock().db).map_err(|e| store_error(&e))?;
        let mut busy = Vec::new();
        for row in rows {
            let tree = id_of(&row)?;
            let path = std::path::Path::new(&row.path);
            if !keep.iter().any(|dir| path.join(dir).exists()) {
                continue;
            }
            let why = if row.state == store::State::Leased.as_str() {
                match self.pause_or_why(&tree) {
                    Err(why) => why,
                    Ok(_) => {
                        "nothing runs in it, but its build directories were not deleted".to_owned()
                    }
                }
            } else if row.state == store::State::Free.as_str() {
                "it is free, but its build directories were not deleted".to_owned()
            } else {
                format!("it is {}", row.state)
            };
            busy.push(BusyTree { tree, why });
        }
        Ok(busy)
    }
}
