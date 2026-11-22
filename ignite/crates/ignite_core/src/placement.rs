//! Comparison of the machines that could take a ticket, using their build outputs and core room.

use crate::capacity::CapacityReport;
use crate::config::Os;
use crate::dispatch::ReadyTicket;
use crate::dispatch_pressure::wants_output;
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
    pub pins: &'a Pins,
}

/// Whether a free or leased tree can donate this output to a new lease.
pub fn holds_output(capacity: &CapacityReport, output: &str) -> bool {
    capacity
        .free_trees
        .iter()
        .chain(capacity.leased_trees.values())
        .any(|tree| tree.iter().any(|held| held == output))
}

pub fn missing_outputs<'a>(
    ready: &ReadyTicket,
    capacity: &'a CapacityReport,
) -> impl Iterator<Item = &'a str> {
    capacity
        .outputs
        .iter()
        .filter(|output| wants_output(output, &ready.files) && !holds_output(capacity, output))
        .map(String::as_str)
}

/// The time to build `outputs` cold on this machine at its currently free cores.
pub fn cold_seconds(capacity: &CapacityReport, outputs: usize) -> f64 {
    COLD_BUILD_CPU_S * outputs as f64 / capacity.free_cores.max(1.0)
}

pub fn free_fraction(capacity: &CapacityReport) -> f64 {
    if capacity.admit_cores > 0.0 {
        (capacity.free_cores / capacity.admit_cores).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn cost(ticket: &PlacementTicket<'_>, machine: &MachineView<'_>) -> Option<f64> {
    let capacity = machine.capacity;
    if machine.state != MachineState::Active
        || capacity.worker_room == 0
        || capacity.memory_pressed
        || !ticket.pins.admits_machine(machine.name)
        || !machine
            .provider
            .is_some_and(|provider| ticket.pins.admits_provider(provider))
        || !ticket.ready.platform_fits(machine.os, machine.families)
    {
        return None;
    }
    Some(
        capacity.queue_seconds
            + cold_seconds(capacity, missing_outputs(ticket.ready, capacity).count()),
    )
}

/// The preferred machine when the asker should defer, or `None` when the asker takes it.
pub fn takes<'a>(
    ticket: &PlacementTicket<'_>,
    asker: &MachineView<'_>,
    peers: &[MachineView<'a>],
    first_deferred: Option<Timestamp>,
    now: Timestamp,
) -> Option<&'a str> {
    preferred_machine(ticket, asker, peers, first_deferred, now).map(|machine| machine.name)
}

/// Return the selected report as well as its name so the caller can explain the comparison.
pub fn preferred_machine<'a, 'peers>(
    ticket: &PlacementTicket<'_>,
    asker: &MachineView<'_>,
    peers: &'peers [MachineView<'a>],
    first_deferred: Option<Timestamp>,
    now: Timestamp,
) -> Option<&'peers MachineView<'a>> {
    if first_deferred.is_some_and(|first| now.seconds_since(first) >= PATIENCE_S) {
        return None;
    }
    let rank = |machine: &MachineView<'_>| {
        cost(ticket, machine).map(|seconds| (seconds, free_fraction(machine.capacity)))
    };
    let (here, here_free) = rank(asker)?;
    let (best, there, there_free) = peers
        .iter()
        .filter_map(|peer| rank(peer).map(|(seconds, free)| (peer, seconds, free)))
        .min_by(
            |(first, first_cost, first_free), (second, second_cost, second_free)| {
                first_cost
                    .total_cmp(second_cost)
                    .then_with(|| second_free.total_cmp(first_free))
                    .then_with(|| first.name.cmp(second.name))
            },
        )?;
    // A small difference in time can start immediately. An exact tie has a stable winner.
    let peer_wins_tie = here.total_cmp(&there).is_eq()
        && there_free
            .total_cmp(&here_free)
            .then_with(|| asker.name.cmp(best.name))
            .is_gt();
    (here - there > MARGIN_S || peer_wins_tie).then_some(best)
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod tests;
