//! Take a cheap snapshot for handoffs. Read actual filesystem headroom, reuse the last sweep's
//! usage measurement, and check whether the known output directories are populated.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use ignite_core::capacity::{BYTES_PER_GB, BYTES_PER_GIB, disk_reserve_bytes};
use ignite_core::dispatch_pressure::{DispatchPressure, TreeCap};
use ignite_core::ids::TicketId;
use ignite_core::traits::Result;

use super::{DiskLimitSource, Kind, TreeConfig, Trees, choice, disk::usage};
use crate::doctor::{Host as _, NativeHost};
use crate::trees::DiskReport;

impl Trees {
    pub(crate) fn request_disk_for_handoff(&self, target_gb: u64) {
        self.inner
            .handoff_disk_target
            .fetch_max(target_gb, Ordering::Relaxed);
    }

    /// Clears free ticket trees until a waiting handoff's disk target is met. `stop` is asked
    /// during the measurement of shared blocks, which gives way to the disk's floor (see
    /// [`Trees::free_unused_disk`]).
    pub(crate) fn reclaim_disk_for_handoff(&self, stop: &dyn Fn() -> bool) -> Result<()> {
        let report = self.reclaim_disk_for_handoff_with(
            || NativeHost.disk_free_gb(&self.inner.config.dir),
            stop,
        )?;
        // A cap refusal uses the same free-tree cleanup. Refresh the usage estimate so it
        // credits the deleted outputs even when filesystem free space was already ample.
        if !report.cleared.is_empty() && self.disk_cap_gb().is_some() {
            self.refresh_disk_usage(stop);
        }
        Ok(())
    }

    pub(super) fn reclaim_disk_for_handoff_with(
        &self,
        free: impl Fn() -> Option<u64>,
        stop: &dyn Fn() -> bool,
    ) -> Result<DiskReport> {
        let target = self.inner.handoff_disk_target.swap(0, Ordering::Relaxed);
        if target == 0 || free().is_none_or(|free| free >= target) {
            return Ok(DiskReport::default());
        }
        crate::steps::step(format!(
            "clearing unused build outputs for a handoff needing {target} GB free"
        ));
        let report = self.free_unused_disk(target, free, stop)?;
        if !report.cleared.is_empty() {
            eprintln!(
                "ignite: cleared build outputs in {} free trees for a ticket needing {target} GB free",
                report.cleared.len()
            );
        }
        Ok(report)
    }

    /// Use the sweep's budget formula with the cached usage estimate.
    pub(crate) fn disk_budget(&self) -> Option<(u64, DiskLimitSource)> {
        let disk = NativeHost.disk_space(&self.inner.config.dir);
        TreeConfig::disk_budget(disk, usage::estimate(self, disk), self.disk_cap_gb())
    }

    pub(crate) fn dispatch_pressure(
        &self,
        queue_seconds: u32,
        recovery_seconds: u32,
    ) -> DispatchPressure {
        let outputs: BTreeSet<_> = self
            .inner
            .config
            .prebuild_outputs
            .get()
            .values()
            .flatten()
            .cloned()
            .collect();
        let disk = NativeHost.disk_space(&self.inner.config.dir);
        let tree_cap = self.disk_cap_gb().map(|limit_gb| TreeCap {
            limit_gb,
            used_bytes: usage::estimate(self, disk)
                .unwrap_or_else(|| u64::from(limit_gb) * BYTES_PER_GB),
        });
        let mut pressure = DispatchPressure {
            free_gb: disk.map(|disk| disk.available / BYTES_PER_GIB),
            disk_reserve_gib: disk
                .map(|disk| disk_reserve_bytes(disk.total).div_ceil(BYTES_PER_GIB)),
            tree_cap,
            queue_seconds,
            recovery_seconds,
            outputs: outputs.into_iter().collect(),
            ..DispatchPressure::default()
        };
        if let Ok(status) = self.status() {
            for tree in status
                .trees
                .into_iter()
                .filter(|tree| tree.kind == Kind::Ticket)
            {
                let present = || {
                    if tree.built_commit.is_none() {
                        return Vec::new();
                    }
                    pressure
                        .outputs
                        .iter()
                        .filter(|output| choice::missing(&tree.path, &[output.as_str()]) == 0)
                        .cloned()
                        .collect()
                };
                if tree.state == "free" {
                    pressure.free_trees.push(present());
                } else if tree.state == "leased"
                    && let Some(ticket) = tree
                        .ticket
                        .as_ref()
                        .and_then(|id| TicketId::new(id.clone()).ok())
                {
                    pressure.leased.insert(ticket, present());
                }
            }
        }
        pressure
    }
}
