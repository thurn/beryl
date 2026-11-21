//! Reserve build growth at handoff, while the worker is still reading and before the scheduler
//! can see any commands. The hub counts its own reservations and claims in the same transaction
//! that decides whether it can make another reservation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::admission::COMPILES_FROM_CORES;
use crate::dispatch::{JUSTICIAR, ReadyTicket, TicketKind};
use crate::ids::TicketId;
use crate::time::Timestamp;

/// The daemon starts reclaiming outputs below this much free disk space.
pub const DISK_FLOOR_GB: u64 = 50;
/// The existing backstop for ordinary handoffs when the build queue stays full.
pub const BACKLOG_SECONDS: u32 = 120;
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
    /// Actual free space on the filesystem holding the worktrees. Unknown space leaves only
    /// the pacing and build-queue checks in force.
    pub free_gb: Option<u64>,
    /// Seconds until a healthy build queue can admit more build tickets.
    pub recovery_seconds: u32,
    /// The oldest wait for a ticket command at the time of the observation.
    pub queue_seconds: u32,
    /// Output directories from the machine's build manifests.
    pub outputs: Vec<String>,
    /// Populated outputs in each available worktree.
    pub free_trees: Vec<Vec<String>>,
    /// Populated outputs in each leased worktree. An empty list describes a cold lease.
    pub leased: BTreeMap<TicketId, Vec<String>>,
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
}

impl From<String> for HandoffWait {
    fn from(reason: String) -> Self {
        Self {
            reason,
            reclaim_to_gb: None,
        }
    }
}

/// The capacity left after counting tickets already reserved or running.
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
            committed_gb: DISK_FLOOR_GB,
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

    /// The first pressure condition keeping a ticket ready, for the pool status line.
    pub fn blocked(&self, ticket: &ReadyTicket) -> Option<HandoffWait> {
        if ticket.labels.iter().any(|label| label == JUSTICIAR) {
            // Emergency work can restore space itself.
            return None;
        }
        let heavy = Self::heavy(ticket.expected_cores, &ticket.files);
        let growth = if !heavy {
            LIGHT_GROWTH_GB
        } else if ticket.expected_cores >= COMPILES_FROM_CORES
            && self.warm_tree(&ticket.files).is_some()
        {
            WARM_GROWTH_GB
        } else {
            COLD_GROWTH_GB
        };
        let required = self.committed_gb.saturating_add(growth);
        if let Some(free) = self.pressure.free_gb
            && free < required
        {
            let room = free.saturating_sub(self.committed_gb);
            return Some(HandoffWait {
                reason: format!(
                    "{} waits for disk space: {free} GB free, {room} GB available above the \
                     {DISK_FLOOR_GB} GB floor after active tickets, {growth} GB needed (rechecking each round)",
                    ticket.id
                ),
                reclaim_to_gb: Some(required),
            });
        }
        if ticket.kind == TicketKind::Regression {
            return None;
        }
        if self.pressure.queue_seconds >= BACKLOG_SECONDS {
            let seconds = self.pressure.queue_seconds;
            return Some(HandoffWait::from(format!(
                "{} waits for the build queue: ticket commands have waited {seconds} s for cores",
                ticket.id
            )));
        }
        if !heavy {
            return None;
        }
        if self.pressure.recovery_seconds > 0 {
            let seconds = self.pressure.recovery_seconds;
            return Some(HandoffWait::from(format!(
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
                return Some(HandoffWait::from(format!(
                    "{} waits for the next build-ticket handoff in {remaining:.0} s",
                    ticket.id
                )));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
