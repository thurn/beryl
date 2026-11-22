//! Derived machine limits and fleet state as shown by `status` and `doctor`.

use ignite_core::capacity::{BYTES_PER_GB, Derived, DiskLimitSource, MachineFacts};
use ignite_core::config::Os;
use ignite_core::fleet::FleetNotice;

use super::{Host, StatusSection};

pub(crate) fn machine_facts(host: &dyn Host, os: Os) -> MachineFacts {
    MachineFacts {
        os,
        logical_cores: host.cores().max(1),
        physical_cores: host.physical_cores(),
        memory_mb: host.memory_mb(),
        disk_total_gb: 0, // Only core-based capacity is derived here.
    }
}

pub(crate) struct CapacitySection<Budget, Room> {
    pub derived: Derived,
    pub disk_budget: Budget,
    pub worker_room: Room,
}

impl<Budget, Room> StatusSection for CapacitySection<Budget, Room>
where
    Budget: Fn() -> Option<(u64, DiskLimitSource)> + Send + Sync,
    Room: Fn() -> u32 + Send + Sync,
{
    fn title(&self) -> &'static str {
        "capacity"
    }

    fn lines(&self) -> Vec<String> {
        let derived = &self.derived;
        let budget = (self.disk_budget)().map_or_else(
            || "tree budget unknown".to_owned(),
            |(bytes, source)| {
                format!(
                    "{} GB tree budget (set by the {source})",
                    bytes / BYTES_PER_GB
                )
            },
        );
        let room = (self.worker_room)();
        vec![format!(
            "{} express cores, {} kept for tip builds, core floors {} tip and {} batch, \
             {} landing trees, {budget}, worker ceiling {}, room to start {room}",
            derived.express_cores,
            derived.tip_build_cores,
            derived.floor.tip,
            derived.floor.batch,
            derived.landing_trees,
            derived.worker_ceiling,
        )]
    }
}

pub(crate) struct FleetSection<Notice>(pub Notice);

impl<Notice> StatusSection for FleetSection<Notice>
where
    Notice: Fn() -> Option<FleetNotice> + Send + Sync,
{
    fn title(&self) -> &'static str {
        "fleet"
    }

    fn lines(&self) -> Vec<String> {
        vec![(self.0)().as_ref().map_or_else(
            || "waiting for the first fleet notice".to_owned(),
            crate::fleet_notice::state_line,
        )]
    }
}
