//! Take a cheap snapshot for handoffs. Read actual filesystem headroom and check whether the
//! known output directories are populated. A recursive size walk on a busy machine can take
//! longer than a build.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use ignite_core::dispatch_pressure::DispatchPressure;
use ignite_core::ids::TicketId;

use super::{Kind, Trees, choice};
use crate::doctor::{Host as _, NativeHost};

impl Trees {
    pub(crate) fn request_disk_for_handoff(&self, target_gb: u64) {
        self.inner
            .handoff_disk_target
            .fetch_max(target_gb, Ordering::Relaxed);
    }

    pub(crate) fn reclaim_disk_for_handoff(&self) -> ignite_core::traits::Result<()> {
        let target = self.inner.handoff_disk_target.swap(0, Ordering::Relaxed);
        let free = || NativeHost.disk_free_gb(&self.inner.config.dir);
        if target == 0 || free().is_none_or(|free| free >= target) {
            return Ok(());
        }
        crate::steps::step(format!(
            "clearing unused build outputs for a handoff needing {target} GB free"
        ));
        let report = self.free_unused_disk(target, free)?;
        if !report.cleared.is_empty() {
            eprintln!(
                "ignite: cleared build outputs in {} free trees for a ticket needing {target} GB free",
                report.cleared.len()
            );
        }
        Ok(())
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
        let mut pressure = DispatchPressure {
            free_gb: NativeHost.disk_free_gb(&self.inner.config.dir),
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
