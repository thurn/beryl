//! Report build slots, output availability and build directory sizes to the hub.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use ignite_core::capacity::BYTES_PER_GIB;
use ignite_core::dispatch_pressure::DispatchPressure;
use ignite_core::ids::TicketId;
use ignite_core::time::Timestamp;

use super::{Kind, Trees, choice, disk, volume_space};

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
        let (slots, ordinary_free, slot_holders) = self.slot_availability();
        let mut pressure = DispatchPressure {
            free_gb: disk.map(|disk| disk.available / BYTES_PER_GIB),
            build_slots: slots.total,
            build_slots_free: ordinary_free,
            slot_holders,
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

/// Measuring all existing outputs can take longer than a heartbeat. Run one scan at a
/// time and reuse its result between scans. Builds measure individual trees separately
/// when enforcing the ceiling.
#[derive(Default)]
pub(crate) struct LargestTree {
    next_scan: Option<Timestamp>,
    pending: Option<Receiver<Option<u64>>>,
    largest: Option<u64>,
}

impl LargestTree {
    pub(crate) fn sample(&mut self, trees: &Trees, now: Timestamp) -> Option<u64> {
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(Some(largest)) => self.largest = Some(largest),
                Err(TryRecvError::Empty) => return self.largest,
                Ok(None) | Err(TryRecvError::Disconnected) => {
                    self.next_scan = Some(now.plus_seconds(60.0));
                }
            }
            self.pending = None;
        }
        if self.next_scan.is_none_or(|next| now >= next) {
            self.next_scan = Some(now.plus_seconds(15.0 * 60.0));
            let (send, receive) = mpsc::channel();
            let trees = trees.clone();
            if thread::Builder::new()
                .name("measure-largest-tree".to_owned())
                .spawn(move || {
                    let largest = trees.build_directories().ok().and_then(|trees| {
                        trees.iter().try_fold(0, |largest, dirs| {
                            disk::size(dirs).ok().map(|bytes| largest.max(bytes))
                        })
                    });
                    let _ = send.send(largest);
                })
                .is_ok()
            {
                self.pending = Some(receive);
            }
        }
        self.largest
    }
}
