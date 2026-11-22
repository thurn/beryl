//! Report current volume and output availability to the hub.

use std::collections::BTreeSet;

use ignite_core::capacity::BYTES_PER_GIB;
use ignite_core::dispatch_pressure::DispatchPressure;
use ignite_core::ids::TicketId;

use super::{Kind, Trees, choice, volume_space};

impl Trees {
    pub(crate) fn dispatch_pressure(
        &self,
        queue_seconds: u32,
        recovery_seconds: u32,
        cpu_percent: Option<u32>,
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
        let disk = volume_space(&self.root());
        let slots = self.build_slots();
        let mut pressure = DispatchPressure {
            free_gb: disk.map(|disk| disk.available / BYTES_PER_GIB),
            build_slots: slots.total,
            build_slots_free: slots.free,
            queue_seconds,
            recovery_seconds,
            cpu_percent,
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
