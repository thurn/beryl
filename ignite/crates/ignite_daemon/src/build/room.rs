//! Waiting for a build slot and for free space on the volume holding the trees.

use std::path::Path;
#[cfg(test)]
use std::sync::Arc;
use std::time::Duration;

use ignite_core::capacity::BYTES_PER_GB;
use ignite_core::fleet::DEFAULT_TREE_CEILING_GB;
use ignite_core::ids::TreeId;
use ignite_core::protocol::host::ExpressBuildStage;
use ignite_core::traits::{BuildResult, Error, Result};

use super::flight::STOP_RECHECK;
use super::{BuildManager, NOT_STARTED, Plan};
use crate::trees::{BuildSlot, Trees, ceiling_bytes, volume_space};

const DISK_WAIT: &str = "disk space:";
pub(super) const RECHECK: Duration = Duration::from_secs(10);

/// The available bytes on the tree volume and a way to wait.
pub trait DiskRoom: Send + Sync {
    fn available(&self, root: &Path) -> Option<u64>;
    fn pause(&self, how_long: Duration);
}

pub(super) struct NativeRoom;

impl DiskRoom for NativeRoom {
    fn available(&self, root: &Path) -> Option<u64> {
        volume_space(root).map(|disk| disk.available)
    }

    fn pause(&self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

impl BuildManager {
    /// A room probe for tests. The live manager reads the tree root from the fleet notice.
    #[cfg(test)]
    #[must_use]
    pub fn with_room(mut self, room: Arc<dyn DiskRoom>) -> Self {
        self.room = Some(room);
        self
    }

    pub(super) fn stopped_waiting(&self, plan: &Plan, blocker: Option<&str>) -> BuildResult {
        let blocker = blocker.map_or_else(String::new, |blocker| {
            format!(" after waiting for {blocker}")
        });
        self.failure(
            &plan.request.product,
            &format!(
                "{NOT_STARTED}stopped waiting to build {} in {}{blocker}: its work was asked to stop or has ended",
                plan.request.product, plan.request.tree
            ),
        )
    }

    pub(super) fn tracks_compiled_outputs(&self, plan: &Plan) -> bool {
        plan.product.uses_build_slot()
            || self
                .tree_control
                .get()
                .and_then(crate::trees::TreeBuildControl::trees)
                .is_some_and(|trees| {
                    plan.product
                        .outputs
                        .iter()
                        .any(|dir| trees.is_compiled_output(Path::new(dir)))
                })
    }

    /// Disk and slot waits happen before the launcher asks for cores.
    pub(super) fn prepare_space(
        &self,
        plan: &Plan,
        stopped: &dyn Fn() -> bool,
    ) -> std::result::Result<Option<BuildSlot>, BuildResult> {
        let trees = self
            .tree_control
            .get()
            .and_then(crate::trees::TreeBuildControl::trees);
        let result = match trees.as_ref() {
            Some(trees) if self.tracks_compiled_outputs(plan) => self
                .prepare_compiled(trees, &plan.request.tree, &plan.product.outputs, stopped)
                .map(Some),
            _ if self.room.is_some() => self
                .wait_for_room(
                    &plan.request.tree,
                    || {
                        (
                            plan.path.clone(),
                            u64::from(DEFAULT_TREE_CEILING_GB) * BYTES_PER_GB / 2,
                        )
                    },
                    stopped,
                )
                .map(|()| None),
            _ => Ok(None),
        };
        result
            .map_err(|error| self.failure(&plan.request.product, &format!("{NOT_STARTED}{error}")))
    }

    fn prepare_compiled(
        &self,
        trees: &Trees,
        tree: &TreeId,
        outputs: &[String],
        stopped: &dyn Fn() -> bool,
    ) -> Result<BuildSlot> {
        let slot = Self::wait_for_slot(trees, tree, outputs, stopped)?;
        let (_, settings) = trees.build_volume();
        trees.prepare_build(tree, ceiling_bytes(&settings), stopped)?;
        self.wait_for_room(
            tree,
            || {
                let (root, settings) = trees.build_volume();
                (root, ceiling_bytes(&settings) / 2)
            },
            stopped,
        )?;
        Ok(slot)
    }

    fn wait_for_slot(
        trees: &Trees,
        tree: &TreeId,
        outputs: &[String],
        stopped: &dyn Fn() -> bool,
    ) -> Result<BuildSlot> {
        let mut waiting = None;
        while !stopped() {
            if let Some(slot) = trees.take_build_slot(tree, outputs, true)? {
                return Ok(slot);
            }
            waiting
                .get_or_insert_with(|| crate::steps::waiting(format!("a build slot for {tree}")));
            std::thread::sleep(STOP_RECHECK);
        }
        Err(Error::Conflict(format!(
            "stopped waiting for a build slot in {tree}"
        )))
    }

    pub(super) fn express_space(
        &self,
        path: &Path,
        output: &Path,
        stage: ExpressBuildStage,
    ) -> Result<()> {
        let Some(trees) = self
            .tree_control
            .get()
            .and_then(crate::trees::TreeBuildControl::trees)
        else {
            return Ok(());
        };
        let Some(tree) = trees.tree_at(path) else {
            return Ok(());
        };
        if trees.path(&tree).as_deref() != Some(path) || !trees.is_compiled_output(output) {
            return Ok(());
        }
        match stage {
            ExpressBuildStage::Prepare => {
                let activity = crate::steps::current();
                let cancelled = || {
                    activity
                        .as_ref()
                        .is_some_and(|activity| activity.cancelled())
                };
                let _slot = self.prepare_compiled(&trees, &tree, &[], &cancelled)?;
                if cancelled() {
                    return Err(Error::Conflict(format!(
                        "the express build in {tree} was stopped"
                    )));
                }
                // The runner is in the verb process. An output directory keeps its slot
                // between this reply and the runner starting Cargo.
                Trees::create_build_output(&tree, path, output)?;
            }
            ExpressBuildStage::Passed => {
                trees.measure_build(&tree, Some(path.join(output).join("express.log")));
            }
            ExpressBuildStage::Failed => trees.measure_build(&tree, None),
        }
        Ok(())
    }

    fn wait_for_room(
        &self,
        tree: &TreeId,
        volume: impl Fn() -> (std::path::PathBuf, u64),
        stopped: &dyn Fn() -> bool,
    ) -> Result<()> {
        let probe = self.room.as_deref().unwrap_or(&NativeRoom);
        let mut waiting = None;
        loop {
            let (root, minimum) = volume();
            let Some(free) = probe.available(&root).filter(|free| *free < minimum) else {
                return Ok(());
            };
            if waiting.is_none() {
                let reason = format!(
                    "{DISK_WAIT} {} GB free, under half the tree ceiling ({} GB)",
                    free / BYTES_PER_GB,
                    minimum / BYTES_PER_GB
                );
                eprintln!("ignite: {tree} waits to build: {reason}");
                waiting = Some(crate::steps::waiting(reason));
            }
            let mut paused = Duration::ZERO;
            while paused < RECHECK {
                if stopped() {
                    return Err(Error::Conflict(format!(
                        "stopped waiting for disk space to build in {tree}"
                    )));
                }
                probe.pause(STOP_RECHECK);
                paused += STOP_RECHECK;
            }
        }
    }
}
