//! The core readings used at startup and by `doctor`.

use ignite_core::capacity::MachineFacts;
use ignite_core::config::Os;

use super::Host;

pub(crate) fn machine_facts(host: &dyn Host, os: Os) -> MachineFacts {
    MachineFacts {
        os,
        logical_cores: host.cores().max(1),
        physical_cores: host.physical_cores(),
        memory_mb: host.memory_mb(),
        disk_total_gb: 0, // Only core-based capacity is derived here.
    }
}
