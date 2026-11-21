//! What the ledger reports about itself: the scheduler's state for `ignite status`, and the
//! grants named when they exceed the machine's cores.

use std::collections::BTreeMap;

use ignite_core::protocol::Class;

use super::super::Status;
use super::{Ledger, Origin};

impl Ledger {
    /// Every grant held, as `(class, cores, adopted)`, for the log line written when the grants
    /// exceed the machine's cores.
    pub fn holders(&self) -> Vec<(Class, u32, bool)> {
        self.held
            .values()
            .map(|h| (h.class, h.cores, h.origin == Origin::Adopted))
            .collect()
    }

    /// The scheduler's state for `ignite status`.
    pub fn status(&self) -> Status {
        let classes = Class::ALL
            .iter()
            .map(|&class| super::super::ClassStatus {
                class,
                running: self.held.values().filter(|h| h.class == class).count() as u32,
                cores: self.class_cores(class),
                waiting: self
                    .waiting
                    .iter()
                    .filter(|w| w.request.class == class)
                    .count() as u32,
            })
            .collect();
        let resources = self
            .limits
            .resources
            .iter()
            .map(|(name, &capacity)| super::super::ResourceStatus {
                name: name.clone(),
                capacity,
                held: self.resource_held(name),
                waiting: self
                    .waiting
                    .iter()
                    .filter(|w| w.request.resources.contains(name))
                    .count() as u32,
            })
            .collect();
        let mut priority_grants = BTreeMap::new();
        for ticket in self
            .held
            .values()
            .filter_map(|h| h.priority_ticket.as_ref())
        {
            *priority_grants.entry(ticket.clone()).or_default() += 1;
        }
        Status {
            cores: self.limits.cores,
            outside: self.outside,
            granted: self.granted_cores(),
            committed: self.committed_cores(),
            used: self
                .held
                .values()
                .filter_map(|h| h.usage.latest())
                .sum::<f64>()
                .round() as u32,
            swap_used_mb: self.pressure.swap_used_mb(),
            memory_pressed: self.memory_pressed(self.free_memory_mb),
            max_per_command: self.limits.max_per_command(),
            free_memory_mb: self.free_memory_mb,
            tip_build: self.limits.tip_build,
            tip_build_used: self.tip_reserve_used(),
            priority_grants,
            classes,
            resources,
        }
    }
}
