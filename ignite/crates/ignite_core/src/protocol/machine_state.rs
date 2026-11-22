//! Measurements sent by the daemon and the dispatch decision recorded by the hub.

use serde::{Deserialize, Serialize};

use crate::capacity::{BYTES_PER_GIB, CapacityReport};
use crate::dispatch_pressure::DispatchPressure;
use crate::ids::{MachineId, TicketId};
use crate::protocol::hub::{Room, WorkerState};
use crate::time::Timestamp;

/// A clone-aware estimate from the most recent completed build directory measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReclaimableDisk {
    pub bytes: u64,
    pub measured_at: Timestamp,
}

/// A heartbeat's machine readings. The hub uses its queue and claims to evaluate starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineSample {
    /// Capacity for build slots, along with the inputs to handoff pacing.
    pub pressure: DispatchPressure,
    pub reclaimable: Option<ReclaimableDisk>,
    pub cpu_percent: Option<u32>,
    pub queued_cores: u64,
    pub swap_used_mb: Option<u64>,
    /// Unoccupied worker slots below the configured ceiling, even during a start block.
    pub idle_workers: u32,
    pub room: Room,
    pub free_ticket_trees: u32,
    pub admit_cores: u32,
    pub draining: bool,
    pub shared_failure: Option<String>,
}

/// What keeps an otherwise eligible ticket from starting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartBlock {
    Disk,
    Cores,
    Pacing,
    Emergency,
    Paused,
    Draining,
    Memory,
    Workers,
    Trees,
    SharedFailure,
}

impl StartBlock {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disk => "disk",
            Self::Cores => "cores",
            Self::Pacing => "pacing",
            Self::Emergency => "emergency",
            Self::Paused => "paused",
            Self::Draining => "draining",
            Self::Memory => "memory",
            Self::Workers => "workers",
            Self::Trees => "trees",
            Self::SharedFailure => "shared_failure",
        }
    }
}

/// One hub-timestamped sample. An absent reading means the source was unavailable.
/// Next-ticket fields describe the first ticket that can start, or the highest-priority
/// eligible ticket if every start is blocked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineSnapshot {
    pub at: Timestamp,
    pub free_disk_gib: Option<u64>,
    pub reclaimable: Option<ReclaimableDisk>,
    pub next_ticket: Option<TicketId>,
    /// Required free space, including the volume reserve and growth reserved by held tickets.
    pub next_ticket_disk_gib: Option<u64>,
    pub start_block: Option<StartBlock>,
    pub start_reason: Option<String>,
    pub cpu_percent: Option<u32>,
    pub queued_cores: u64,
    pub swap_used_mb: Option<u64>,
    pub idle_workers: u32,
    /// Includes workers with a reservation who are about to start.
    pub working_workers: u32,
    pub stalled_workers: u32,
    pub down_workers: u32,
    /// Unclaimed, unreserved work eligible for the machine and its providers before checking
    /// transient start limits. This count can remain positive while starts are blocked.
    pub ready_tickets: u32,
}

impl MachineSnapshot {
    /// Shared display values for status and the dashboard.
    pub fn readings(&self) -> [(&'static str, String); 5] {
        let free = self
            .free_disk_gib
            .map_or_else(|| "unknown".to_owned(), |value| format!("{value} GiB"));
        let reclaimable = self.reclaimable.map_or_else(
            || "reclaimable disk unmeasured".to_owned(),
            |disk| {
                format!(
                    "{:.1} GiB reclaimable (measured {})",
                    disk.bytes as f64 / BYTES_PER_GIB as f64,
                    disk.measured_at
                )
            },
        );
        let cpu = self
            .cpu_percent
            .map_or_else(|| "unknown".to_owned(), |value| format!("{value}%"));
        let swap = self
            .swap_used_mb
            .map_or_else(|| "unknown".to_owned(), |value| format!("{value} MB"));
        let next = self.next_ticket.as_ref().map_or_else(
            || "No eligible ticket".to_owned(),
            |ticket| {
                self.next_ticket_disk_gib.map_or_else(
                    || ticket.to_string(),
                    |gib| format!("{ticket} needs {gib} GiB free"),
                )
            },
        );
        let starts = if let Some(reason) = &self.start_reason {
            format!("{next}. {reason}")
        } else {
            next
        };
        [
            ("Disk", format!("{free} free, {reclaimable}")),
            (
                "CPU and swap",
                format!("{cpu} CPU, {} queued cores, {swap} swap", self.queued_cores),
            ),
            (
                "Workers",
                format!(
                    "{} idle, {} working, {} stalled, {} down",
                    self.idle_workers,
                    self.working_workers,
                    self.stalled_workers,
                    self.down_workers
                ),
            ),
            ("Ready tickets", self.ready_tickets.to_string()),
            ("Next start", starts),
        ]
    }
}

/// `POST /machines/heartbeat`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Heartbeat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_volumes: Option<crate::fleet::TreeVolumes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<MachineSample>,
    pub machine: MachineId,
    pub version: String,
    pub workers: Vec<WorkerState>,
    /// Providers configured to run workers, even when they offer no slots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cli_apps: Vec<String>,
    /// The check families the machine's `ignite.toml` lists for it now, empty for every
    /// family, which replace the list it registered with. `None` from a daemon that predates
    /// the field, which leaves the list the hub holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub families: Option<Vec<String>>,
    /// Capacity measured with the machine sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<CapacityReport>,
}
