//! Reserve build growth at handoff, while the worker is still reading and before the scheduler
//! can see any commands. The hub counts its own reservations and claims in the same transaction
//! that decides whether it can make another reservation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::admission::COMPILES_FROM_CORES;
use crate::capacity::{BYTES_PER_GB, BYTES_PER_GIB};
use crate::dispatch::{JUSTICIAR, ReadyTicket, TicketKind};
use crate::ids::TicketId;
use crate::protocol::machine_state::StartBlock;
use crate::time::Timestamp;

/// The daemon starts reclaiming outputs below this much free disk space.
pub const DISK_FLOOR_GB: u64 = 50;
/// How long commands have to wait on a busy host before ordinary handoffs stop.
pub const BACKLOG_SECONDS: u32 = 120;
/// At 90% CPU, an 18-core host has about two cores left. On that host the queue often
/// reached two minutes with much more headroom, while grants were still counted in full.
const BACKLOG_BUSY_PERCENT: u32 = 90;
/// Worktree build outputs can cost tens of GB to reconstruct on this machine class. Already
/// populated outputs still need space for changed objects and incremental caches.
const COLD_GROWTH_GB: u64 = 40;
const WARM_GROWTH_GB: u64 = 12;
const LIGHT_GROWTH_GB: u64 = 1;
const INITIAL_BUILD_TICKETS: usize = 3;
const RAMP_SECONDS: u32 = 90;
const BUILD_WAIT_SECONDS: f64 = 30.0;
const RECOVERY_SECONDS: f64 = 90.0;

/// Watch the queue independently of available workers. A short drop in wait time does not
/// let more build tickets into a queue that is only starting to drain.
#[derive(Default)]
pub struct QueueRecovery {
    until: Option<Timestamp>,
}

impl QueueRecovery {
    pub fn observe(&mut self, now: Timestamp, waited: f64) -> u32 {
        if waited >= BUILD_WAIT_SECONDS {
            self.until = Some(now.plus_seconds(RECOVERY_SECONDS));
        }
        self.until
            .map_or(0, |deadline| deadline.seconds_since(now).ceil() as u32)
    }
}

/// Capacity observed by the machine just before asking for another handoff.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchPressure {
    /// Actual free space on the filesystem holding the worktrees, in GiB.
    pub free_gb: Option<u64>,
    /// Free space the automatic tree budget leaves outside the trees, rounded up to GiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_reserve_gib: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_cap: Option<TreeCap>,
    /// Seconds until a healthy build queue can admit more build tickets.
    pub recovery_seconds: u32,
    /// The oldest wait for a ticket command at the time of the observation.
    pub queue_seconds: u32,
    /// Recent measured utilization of all logical cores, including work outside Ignite.
    /// Older daemons and hosts without a fresh reading still use the wait alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<u32>,
    /// Output directories from the machine's build manifests.
    pub outputs: Vec<String>,
    /// Populated outputs in each available worktree.
    pub free_trees: Vec<Vec<String>>,
    /// Populated outputs in each leased worktree. An empty list describes a cold lease.
    pub leased: BTreeMap<TicketId, Vec<String>>,
}

impl DispatchPressure {
    pub fn queue_backlogged(&self) -> bool {
        self.queue_seconds >= BACKLOG_SECONDS
            && self
                .cpu_percent
                .is_none_or(|busy| busy >= BACKLOG_BUSY_PERCENT)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeCap {
    /// Fleet caps use decimal GB.
    pub limit_gb: u32,
    /// Estimated allocated bytes in all managed build outputs.
    pub used_bytes: u64,
}

/// Whether a build output directory is beside one of the ticket's files.
pub fn wants_output(output: &str, files: &[String]) -> bool {
    let parent = output.rsplit_once('/').map_or("", |(parent, _)| parent);
    let within = |path: &str, dir: &str| {
        dir.is_empty()
            || path
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    files
        .iter()
        .any(|file| within(file, parent) && !within(file, output))
}

/// The condition preventing a handoff, and how much disk the machine can try to reclaim.
pub struct HandoffWait {
    pub reason: String,
    pub reclaim_to_gb: Option<u64>,
    pub block: Option<StartBlock>,
}

impl From<String> for HandoffWait {
    fn from(reason: String) -> Self {
        Self {
            reason,
            reclaim_to_gb: None,
            block: None,
        }
    }
}

/// The capacity left after counting tickets already reserved or running.
#[derive(Clone)]
pub struct Budget<'a> {
    pressure: &'a DispatchPressure,
    committed_gb: u64,
    free_trees: Vec<&'a Vec<String>>,
    heavy: usize,
    latest_heavy: Option<Timestamp>,
    now: Timestamp,
}

impl<'a> Budget<'a> {
    pub fn new(pressure: &'a DispatchPressure, now: Timestamp) -> Self {
        Self {
            pressure,
            committed_gb: 0,
            free_trees: pressure.free_trees.iter().collect(),
            heavy: 0,
            latest_heavy: None,
            now,
        }
    }

    fn heavy(cores: f64, files: &[String]) -> bool {
        cores >= COMPILES_FROM_CORES
            || files.iter().any(|file| {
                let path = std::path::Path::new(file);
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                    || path.file_name().is_some_and(|name| name == "Cargo.toml")
            })
    }

    fn warm(&self, outputs: &[String], files: &[String]) -> bool {
        let mut wanted = self
            .pressure
            .outputs
            .iter()
            .filter(|output| wants_output(output, files));
        wanted.next().is_some_and(|first| {
            outputs.contains(first) && wanted.all(|output| outputs.contains(output))
        })
    }

    fn warm_tree(&self, files: &[String]) -> Option<usize> {
        self.free_trees
            .iter()
            .position(|outputs| self.warm(outputs, files))
    }

    /// Every held ticket retains a growth allowance for the rest of its work. Pending
    /// reservations consume available warm trees at most once.
    pub fn hold(
        &mut self,
        ticket: &TicketId,
        cores: f64,
        files: &[String],
        since: Option<Timestamp>,
    ) {
        let growth = if Self::heavy(cores, files) {
            self.heavy += 1;
            self.latest_heavy = self.latest_heavy.max(since);
            let warm = if let Some(outputs) = self.pressure.leased.get(ticket) {
                self.warm(outputs, files)
            } else if cores >= COMPILES_FROM_CORES
                && let Some(index) = self.warm_tree(files)
            {
                self.free_trees.swap_remove(index);
                true
            } else {
                false
            };
            if warm { WARM_GROWTH_GB } else { COLD_GROWTH_GB }
        } else {
            LIGHT_GROWTH_GB
        };
        self.committed_gb = self.committed_gb.saturating_add(growth);
    }

    fn growth(&self, ticket: &ReadyTicket) -> u64 {
        if !Self::heavy(ticket.expected_cores, &ticket.files) {
            LIGHT_GROWTH_GB
        } else if ticket.expected_cores >= COMPILES_FROM_CORES
            && self.warm_tree(&ticket.files).is_some()
        {
            WARM_GROWTH_GB
        } else {
            COLD_GROWTH_GB
        }
    }

    /// Free space needed to start this ticket, including space reserved by active tickets.
    pub fn disk_target_gib(&self, ticket: &ReadyTicket) -> u64 {
        if ticket.labels.iter().any(|label| label == JUSTICIAR) {
            return 0;
        }
        let volume_target = self
            .pressure
            .disk_reserve_gib
            .unwrap_or(0)
            .max(DISK_FLOOR_GB)
            .saturating_add(self.committed_gb)
            .saturating_add(self.growth(ticket));
        self.disk_wait(ticket)
            .and_then(|wait| wait.reclaim_to_gb)
            .map_or(volume_target, |target| target.max(volume_target))
    }

    fn disk_wait(&self, ticket: &ReadyTicket) -> Option<HandoffWait> {
        let growth = self.growth(ticket);
        let required = self.committed_gb.saturating_add(growth);
        let floor_target = DISK_FLOOR_GB.saturating_add(required);
        if let Some(free) = self.pressure.free_gb
            && free < floor_target
        {
            let room = free.saturating_sub(DISK_FLOOR_GB.saturating_add(self.committed_gb));
            return Some(HandoffWait {
                block: Some(StartBlock::Disk),
                reason: format!(
                    "{} waits for disk space: {free} GB free, {room} GB available above the \
                     {DISK_FLOOR_GB} GB floor after active tickets, {growth} GB needed (rechecking each round)",
                    ticket.id
                ),
                reclaim_to_gb: Some(floor_target),
            });
        }
        if let Some(reserve) = self.pressure.disk_reserve_gib
            && let Some(free) = self.pressure.free_gb
            && free < reserve.saturating_add(required)
        {
            let room = free.saturating_sub(reserve.saturating_add(self.committed_gb));
            return Some(HandoffWait {
                block: Some(StartBlock::Disk),
                reason: format!(
                    "{} waits for the automatic tree disk budget: {room} GiB available after \
                     the {reserve} GiB volume reserve and active tickets, {growth} GiB needed",
                    ticket.id
                ),
                reclaim_to_gb: Some(reserve.saturating_add(required)),
            });
        }
        if let Some(cap) = self.pressure.tree_cap {
            let limit = u64::from(cap.limit_gb) * BYTES_PER_GB;
            let projected = cap
                .used_bytes
                .saturating_add(required.saturating_mul(BYTES_PER_GIB));
            if projected > limit {
                let room = (limit.saturating_sub(cap.used_bytes) / BYTES_PER_GIB)
                    .saturating_sub(self.committed_gb);
                // Free-tree cleanup can create cap headroom as well as filesystem headroom.
                // Skip it when even deleting every output would leave too little room.
                let reclaim_to_gb = self
                    .pressure
                    .free_gb
                    .filter(|_| required <= limit / BYTES_PER_GIB)
                    .map(|free| free.saturating_add((projected - limit).div_ceil(BYTES_PER_GIB)));
                return Some(HandoffWait {
                    block: Some(StartBlock::Disk),
                    reason: format!(
                        "{} waits for the Fleet tree disk cap: {room} GiB available after active \
                         tickets, {growth} GiB needed under the {} GB cap",
                        ticket.id, cap.limit_gb
                    ),
                    reclaim_to_gb,
                });
            }
        }
        None
    }

    /// The first pressure condition keeping a ticket ready, for the pool status line.
    pub fn blocked(&self, ticket: &ReadyTicket) -> Option<HandoffWait> {
        if ticket.labels.iter().any(|label| label == JUSTICIAR) {
            // Emergency work can restore space itself.
            return None;
        }
        if let Some(wait) = self.disk_wait(ticket) {
            return Some(wait);
        }
        if ticket.kind == TicketKind::Regression {
            return None;
        }
        if self.pressure.queue_backlogged() {
            let seconds = self.pressure.queue_seconds;
            return Some(pacing(format!(
                "{} waits for the build queue: ticket commands have waited {seconds} s for cores",
                ticket.id
            )));
        }
        if !Self::heavy(ticket.expected_cores, &ticket.files) {
            return None;
        }
        if self.pressure.recovery_seconds > 0 {
            let seconds = self.pressure.recovery_seconds;
            return Some(pacing(format!(
                "{} waits for a healthy build queue: rechecking each round, up to {seconds} s \
                 of recovery left once waits subside",
                ticket.id
            )));
        }
        if self.heavy >= INITIAL_BUILD_TICKETS
            && let Some(latest) = self.latest_heavy
        {
            let remaining = f64::from(RAMP_SECONDS) - self.now.seconds_since(latest).max(0.0);
            if remaining > 0.0 {
                return Some(pacing(format!(
                    "{} waits for the next build-ticket handoff in {remaining:.0} s",
                    ticket.id
                )));
            }
        }
        None
    }
}

fn pacing(reason: String) -> HandoffWait {
    HandoffWait {
        reason,
        reclaim_to_gb: None,
        block: Some(StartBlock::Pacing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_queue_only_blocks_ordinary_handoffs_when_the_host_is_busy_or_unmeasured() {
        let mut pressure = DispatchPressure {
            queue_seconds: BACKLOG_SECONDS - 1,
            cpu_percent: Some(100),
            ..DispatchPressure::default()
        };
        assert!(!pressure.queue_backlogged());

        pressure.queue_seconds = BACKLOG_SECONDS;
        pressure.cpu_percent = Some(89);
        assert!(!pressure.queue_backlogged());
        pressure.cpu_percent = Some(90);
        assert!(pressure.queue_backlogged());

        let mut old_sample = serde_json::to_value(&pressure).unwrap();
        old_sample.as_object_mut().unwrap().remove("cpu_percent");
        let old_pressure: DispatchPressure = serde_json::from_value(old_sample).unwrap();
        assert!(old_pressure.queue_backlogged());
    }

    #[test]
    fn a_new_long_wait_restarts_the_recovery_interval() {
        let mut queue = QueueRecovery::default();
        let start = Timestamp::EPOCH;
        assert_eq!(queue.observe(start, 29.0), 0);
        assert_eq!(queue.observe(start, 30.0), 90);
        assert_eq!(queue.observe(start.plus_seconds(89.0), 0.0), 1);
        assert_eq!(queue.observe(start.plus_seconds(89.0), 32.0), 90);
        assert_eq!(queue.observe(start.plus_seconds(90.0), 0.0), 89);
        assert_eq!(queue.observe(start.plus_seconds(179.0), 0.0), 0);
    }
}
