//! Comparison of the machines that could take a ticket.
//!
//! The ticket that places work where it will build soonest implements the comparisons here.

#![allow(unused_variables)]

use crate::capacity::CapacityReport;
use crate::config::Os;
use crate::dispatch::ReadyTicket;
use crate::fleet::MachineState;
use crate::pins::Pins;
use crate::time::Timestamp;

/// Small differences in build time need not delay a hand-off.
pub const MARGIN_S: f64 = 30.0;
/// After this wait, an eligible machine may take a ticket that was passed over.
pub const PATIENCE_S: f64 = 60.0;
/// Older measurements can misrepresent current room and build outputs.
pub const REPORT_MAX_AGE_S: f64 = 60.0;
/// The estimated CPU time to build one missing output.
pub const COLD_BUILD_CPU_S: f64 = 1_000.0;
/// Start a few workers at once so their memory use is visible before more start.
pub const START_BURST: u32 = 3;
/// Use this footprint until there are measurements of worker turns.
pub const WORKER_MB_DEFAULT: u64 = 2_000;
/// Keep two free trees as warm donors for new leases.
pub const FREE_TREE_KEEP: usize = 2;
/// Retire surplus donors after a day without a lease.
pub const FREE_TREE_RETIRE_HOURS: u32 = 24;
/// Leave this much space for growth outside the build trees, even on small disks.
pub const DISK_RESERVE_GB: u64 = 50;
/// Keep more space for growth outside the trees on large disks.
pub const DISK_RESERVE_FRACTION: f64 = 0.1;

pub struct MachineView<'a> {
    pub name: &'a str,
    pub os: Os,
    pub families: &'a [String],
    pub state: MachineState,
    pub capacity: &'a CapacityReport,
    /// The provider chosen for the ticket on this machine, if one can take it.
    pub provider: Option<&'a str>,
}

pub struct PlacementTicket<'a> {
    pub ready: &'a ReadyTicket,
    pub needs_os: Option<Os>,
    pub pins: &'a Pins,
}

pub fn cost(ticket: &PlacementTicket<'_>, machine: &MachineView<'_>) -> Option<f64> {
    todo!()
}

/// The preferred machine when the asker should defer, or `None` when the asker takes it.
pub fn takes<'a>(
    ticket: &PlacementTicket<'_>,
    asker: &MachineView<'_>,
    peers: &[MachineView<'a>],
    first_deferred: Option<Timestamp>,
    now: Timestamp,
) -> Option<&'a str> {
    todo!()
}
