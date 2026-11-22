//! Capacity derived from machine measurements and reported to the hub.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::config::Os;
use crate::ids::TicketId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineFacts {
    pub os: Os,
    pub logical_cores: u32,
    pub physical_cores: u32,
    pub memory_mb: u64,
    pub disk_total_gb: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Derived {
    pub express_cores: u32,
    pub tip_build_cores: u32,
    pub floor: Floor,
    pub landing_trees: u32,
    pub worker_ceiling: u32,
}

/// Cores reserved for the `batch` and `tip` classes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Floor {
    pub batch: u32,
    pub tip: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomInputs {
    pub ceiling: u32,
    pub running: u32,
    /// Starts whose footprint may still be missing from the free-memory sample.
    pub starting: u32,
    pub memory_pressed: bool,
    pub free_memory_mb: Option<u64>,
    pub worker_mb: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CapacityReport {
    pub logical_cores: u32,
    pub physical_cores: u32,
    pub memory_mb: u64,
    pub admit_cores: f64,
    pub free_cores: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outside_cores: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_workers: Option<u32>,
    pub worker_room: u32,
    pub free_memory_mb: Option<u64>,
    pub memory_pressed: bool,
    /// Free volume space in GiB.
    pub free_disk_gb: Option<u64>,
    /// Kept on the wire for hubs that still require this field.
    pub disk_budget_gb: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_budget_source: Option<DiskLimitSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_space: Option<BuildSpaceReport>,
    /// Leases already included in the occupied count of the build-space report.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub slot_holders: BTreeSet<TicketId>,
    pub queue_seconds: f64,
    /// Configured build output directories, whether or not any tree holds them.
    pub outputs: Vec<String>,
    pub free_trees: Vec<Vec<String>>,
    pub leased_trees: BTreeMap<TicketId, Vec<String>>,
    pub providers: Vec<ProviderCapacity>,
}

/// Capacity and sizes of the machine's managed build trees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSpaceReport {
    pub slots: u32,
    pub free: u32,
    pub ceiling_gb: u32,
    pub largest_tree_bytes: Option<u64>,
}

impl std::fmt::Display for BuildSpaceReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} build slots, {} free, {} GB tree ceiling",
            self.slots, self.free, self.ceiling_gb
        )?;
        if let Some(bytes) = self.largest_tree_bytes {
            write!(
                f,
                ", largest tree {:.1} GB",
                bytes as f64 / BYTES_PER_GB as f64
            )
        } else {
            f.write_str(", largest tree unmeasured")
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapacity {
    pub name: String,
    pub tiers: Vec<String>,
    pub probed: bool,
    pub hold_reason: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_ids: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

/// The term that set the limit for a sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiskLimitSource {
    Cap,
    Disk,
    FreeSpaceFloor,
}

impl std::fmt::Display for DiskLimitSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cap => "cap",
            Self::Disk => "disk",
            Self::FreeSpaceFloor => "free-space floor",
        })
    }
}

pub fn derive(facts: &MachineFacts) -> Derived {
    let physical = facts.physical_cores;
    let express_cores = round_div(physical, 6).clamp(2, 4);
    Derived {
        express_cores,
        tip_build_cores: round_div(physical, 5).clamp(2, 6),
        floor: Floor {
            batch: round_div(physical, 3).max(2),
            tip: 2,
        },
        landing_trees: express_cores,
        worker_ceiling: facts.logical_cores.saturating_mul(2),
    }
}

/// Round to the nearest integer, with halves rounding up.
fn round_div(value: u32, divisor: u32) -> u32 {
    value / divisor + u32::from(value % divisor >= divisor.div_ceil(2))
}

/// Capacity for a resource whose configuration gives slots per logical core.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn resource_slots(logical_cores: u32, per_core: f64) -> u32 {
    (f64::from(logical_cores) * per_core).floor().max(1.0) as u32
}

/// Fleet tree sizes use decimal gigabytes.
pub const BYTES_PER_GB: u64 = 1_000_000_000;
/// Free disk readings use gibibytes.
pub const BYTES_PER_GIB: u64 = 1 << 30;
/// Memory left for other processes after starting a worker, in megabytes.
const WORKER_MEMORY_RESERVE_MB: u64 = 1_024;

/// The largest batch of workers one dispatch step can start.
pub const WORKER_BATCH: u32 = 3;

/// How many more worker footprints fit after reserving memory for the system and pending starts.
pub fn worker_memory_room(inputs: &RoomInputs) -> u32 {
    if inputs.memory_pressed {
        return 0;
    }
    let Some(free) = inputs.free_memory_mb else {
        return u32::MAX;
    };
    if free < WORKER_MEMORY_RESERVE_MB {
        return 0;
    }
    let worker_room = (free - WORKER_MEMORY_RESERVE_MB)
        .checked_div(inputs.worker_mb)
        .unwrap_or(u64::MAX);
    u32::try_from(worker_room.saturating_sub(u64::from(inputs.starting))).unwrap_or(u32::MAX)
}

/// How many more ordinary workers the daemon can start in one dispatch step.
pub fn start_room(inputs: &RoomInputs) -> u32 {
    inputs
        .ceiling
        .saturating_sub(inputs.running.saturating_add(inputs.starting))
        .min(worker_memory_room(inputs))
        .min(WORKER_BATCH)
}

#[cfg(test)]
mod room_tests {
    use super::*;

    #[test]
    fn worker_room_obeys_memory_and_ceiling() {
        let mut inputs = RoomInputs {
            ceiling: 10,
            running: 5,
            starting: 0,
            memory_pressed: false,
            free_memory_mb: Some(3_024),
            worker_mb: 2_000,
        };
        assert_eq!(start_room(&inputs), 1);
        inputs.free_memory_mb = Some(5_024);
        assert_eq!(start_room(&inputs), 2);
        inputs.starting = 1;
        assert_eq!(start_room(&inputs), 1);
        inputs.starting = 0;
        inputs.free_memory_mb = Some(3_023);
        assert_eq!(start_room(&inputs), 0);
        inputs.free_memory_mb = None;
        assert_eq!(start_room(&inputs), 3);
        inputs.running = 9;
        assert_eq!(start_room(&inputs), 1);
        inputs.running = 11;
        assert_eq!(start_room(&inputs), 0);
        inputs.running = 0;
        inputs.memory_pressed = true;
        assert_eq!(start_room(&inputs), 0);
    }
}

#[cfg(test)]
mod tests;
