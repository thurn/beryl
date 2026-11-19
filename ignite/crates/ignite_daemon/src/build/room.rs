//! Waiting for free disk space before a build starts.
//!
//! On 2026-11-15 the disk of the machine running the pool filled to 100% and builds failed with
//! `ENOSPC` while writing their target directories. A backup had taken an APFS snapshot of the
//! volume and held it for hours. While a snapshot exists, every block a build rewrites or deletes
//! stays allocated, so the disk loop's sweeps of free trees (`trees/disk.rs`) freed almost
//! nothing. A build started then fails partway, and so does every other command writing to the
//! disk at the time.
//!
//! So before a build runs, the manager reads the free space on the disk that holds the trees.
//! Below [`START_GB`] it waits, reading again every [`RECHECK`], until there is room or the work
//! that asked for the build is stopped. The disk loop goes on sweeping free trees meanwhile
//! (every five minutes while the disk is below its floor), and the space a snapshot pins comes
//! back once the snapshot is deleted. A disk whose free space cannot be read never holds a build.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ignite_core::traits::BuildResult;

use super::flight::STOP_RECHECK;
use super::{BuildManager, Plan};
use crate::doctor::Host as _;

/// Free disk space below which a build waits instead of starting. The disk loop sweeps free trees
/// whenever the disk has less than 50 GB free, so builds normally start with more than this; the
/// disk falls under it only when sweeping cannot help, as when a snapshot pins what was deleted
/// or every tree is leased. A tree's build directories hold 15 to 50 GB once built, a build from
/// scratch writes most of that, and several trees build at once, so starting one with less than
/// this risks filling the disk.
pub const START_GB: u64 = 30;

/// How often a build waiting for disk space reads the free space again.
pub const RECHECK: Duration = Duration::from_secs(10);

/// The free space on the disk that holds the trees, and a way to wait.
pub trait DiskRoom: Send + Sync {
    /// Gigabytes free, or `None` when they cannot be read.
    fn free_gb(&self) -> Option<u64>;
    /// Blocks for `how_long`. Tests give a fake that returns at once.
    fn pause(&self, how_long: Duration);
}

/// The disk that holds `path`, read with `df`.
pub struct NativeRoom {
    pub path: PathBuf,
}

impl DiskRoom for NativeRoom {
    fn free_gb(&self) -> Option<u64> {
        crate::doctor::NativeHost.disk_free_gb(&self.path)
    }

    fn pause(&self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

impl BuildManager {
    /// Makes builds wait while `room` has less than [`START_GB`] free. Without it builds never
    /// wait for disk space.
    #[must_use]
    pub fn with_room(mut self, room: Arc<dyn DiskRoom>) -> Self {
        self.room = Some(room);
        self
    }

    /// The result of a request whose work was asked to stop while it waited for another build.
    pub(super) fn stopped_waiting(&self, plan: &Plan) -> BuildResult {
        self.failure(
            &plan.request.product,
            &format!(
                "stopped waiting for a build of {} in {}: its work was asked to stop",
                plan.request.product, plan.request.tree
            ),
        )
    }

    /// Returns once the disk has [`START_GB`] free, or with a failure once `stop_requested`
    /// answers true while it waits.
    pub(super) fn wait_for_room(
        &self,
        plan: &Plan,
        stop_requested: &dyn Fn() -> bool,
    ) -> Option<BuildResult> {
        let room = self.room.as_ref()?;
        let mut waiting = None;
        loop {
            let free = room.free_gb()?;
            if free >= START_GB {
                return None;
            }
            if waiting.is_none() {
                eprintln!(
                    "ignite: the build of {} in {} waits for disk space: {free} GB free, under \
                     the {START_GB} GB a build needs to start",
                    plan.request.product, plan.request.tree
                );
                waiting = Some(crate::steps::waiting(format!(
                    "disk space: {free} GB free, under the {START_GB} GB a build needs"
                )));
            }
            let mut paused = Duration::ZERO;
            while paused < RECHECK {
                if stop_requested() {
                    return Some(self.failure(
                        &plan.request.product,
                        &format!(
                            "stopped waiting for disk space to build {} in {}: its work was \
                             asked to stop",
                            plan.request.product, plan.request.tree
                        ),
                    ));
                }
                room.pause(STOP_RECHECK);
                paused += STOP_RECHECK;
            }
        }
    }
}
