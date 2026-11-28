//! Count build tickets at handoff, before their trees have acquired build slots.
//! The hub counts its own reservations and claims in the transaction that makes a reservation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::admission::COMPILES_FROM_CORES;
use crate::dispatch::{JUSTICIAR, ReadyTicket, TicketKind};
use crate::ids::TicketId;
use crate::protocol::machine_state::StartBlock;
use crate::time::Timestamp;

/// How long commands have to wait on a busy host before ordinary handoffs stop.
pub const BACKLOG_SECONDS: u32 = 120;
/// Identifies the build limit in machine snapshots that record the reason as text.
pub const BUILD_LIMIT_WAIT: &str = "waits for shared build cores";
/// Identifies a build-slot wait in snapshots, which also use `Trees` for ticket-tree waits.
pub const BUILD_SLOT_WAIT: &str = "waits for a build slot";
/// Identifies a backlog that is blocking starts in machine snapshots.
pub const BUILD_QUEUE_WAIT: &str = "waits for the build queue";
/// Identifies a backlog recovery wait in machine snapshots.
pub const BUILD_QUEUE_RECOVERY_WAIT: &str = "waits for a healthy build queue";
/// At 90% CPU, an 18-core host has about two cores left. On that host the queue often
/// reached two minutes with much more headroom, while grants were still counted in full.
const BACKLOG_BUSY_PERCENT: u32 = 90;
const INITIAL_BUILD_TICKETS: u32 = 3;
// Build commands overlap in bursts. At 2.0 shared cores per ticket, three concurrent heavy
// client builds on a six-shared-core host drove 13+ queued cores and 3+ GiB of swap, stretching
// single landings past the interval between staging commits. At 2.5, a burst of ten bug
// reports put 8 build tickets on an idle 20-core Mac before its builds began. CPU then sat at
// 100% with 29 cores queued, commands waited ten minutes for cores and tip verification
// stalled behind them. At 3.0, a 6-core host caps at 2 build tickets, a 9-core host at 3 and
// a 20-core host at 6.
const SHARED_CORES_PER_BUILD_TICKET: f64 = 3.0;
// A worker reads its ticket for a few minutes before its first build, so the queue a build
// ticket adds shows up late. 150 s between handoffs after the first three gives the host's
// measurements more time to catch up than the earlier 90 s did.
const RAMP_SECONDS: u32 = 150;
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
    /// Total build slots, including the slot reserved for landing trees.
    #[serde(default)]
    pub build_slots: u32,
    /// Slots available for ordinary ticket builds, including reclaimable ones.
    #[serde(default)]
    pub build_slots_free: u32,
    /// Scheduler cores shared with agent work. Carries the reading on lane successor requests,
    /// which do not include a `MachineFree`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_cores: Option<u32>,
    /// Actual free space on the filesystem holding the worktrees, in GiB.
    pub free_gb: Option<u64>,
    /// Tickets whose leased trees already occupy a slot, including builds in progress.
    /// Daemons that report counts alone may temporarily leave fewer slots available for handoff.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub slot_holders: BTreeSet<TicketId>,
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

    pub fn backlog_reason(&self) -> String {
        format!(
            "{BUILD_QUEUE_WAIT}: ticket commands have waited {} s for cores",
            self.queue_seconds
        )
    }
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

/// Estimates build work from expected cores and Rust paths.
///
/// Tickets without paths count as build work because their build requirements are unknown.
/// Bug reports often omit paths before triage and still need to compile their fixes.
pub fn likely_compiles(cores: f64, files: &[String]) -> bool {
    files.is_empty()
        || cores >= COMPILES_FROM_CORES
        || files.iter().any(|file| {
            let path = std::path::Path::new(file);
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                || path.file_name().is_some_and(|name| name == "Cargo.toml")
        })
}

/// Reasons a handoff is waiting, with the first condition as the primary block.
pub struct HandoffWait {
    pub reason: String,
    pub block: Option<StartBlock>,
}

impl From<String> for HandoffWait {
    fn from(reason: String) -> Self {
        Self {
            reason,
            block: None,
        }
    }
}

/// The capacity left after counting tickets already reserved or running.
#[derive(Clone)]
pub struct Budget<'a> {
    pressure: &'a DispatchPressure,
    pending_slots: u32,
    heavy: u32,
    build_limit: Option<u32>,
    latest_heavy: Option<Timestamp>,
    now: Timestamp,
}

impl<'a> Budget<'a> {
    pub fn new(
        pressure: Option<&'a DispatchPressure>,
        admit_cores: Option<f64>,
        now: Timestamp,
    ) -> Self {
        static UNREPORTED: LazyLock<DispatchPressure> = LazyLock::new(DispatchPressure::default);
        let pressure = pressure.unwrap_or(&UNREPORTED);
        let build_limit = admit_cores
            .or_else(|| pressure.shared_cores.map(f64::from))
            .map(|cores| ((cores / SHARED_CORES_PER_BUILD_TICKET).floor() as u32).max(1));
        Self {
            pressure,
            pending_slots: 0,
            heavy: 0,
            build_limit,
            latest_heavy: None,
            now,
        }
    }

    /// An occupied slot counts as build work even when the ticket was expected to be light.
    /// The daemon's free slot count already accounts for it. Returns whether the ticket counts
    /// as a build ticket against [`Budget::build_limit`].
    pub fn hold(
        &mut self,
        ticket: &TicketId,
        needs_build_slot: bool,
        since: Option<Timestamp>,
    ) -> bool {
        let occupies_slot = self.pressure.slot_holders.contains(ticket);
        if occupies_slot || needs_build_slot {
            self.heavy += 1;
            self.latest_heavy = self.latest_heavy.max(since);
            if !occupies_slot {
                self.pending_slots = self.pending_slots.saturating_add(1);
            }
        }
        occupies_slot || needs_build_slot
    }

    /// How many build tickets the machine may hold: one per `SHARED_CORES_PER_BUILD_TICKET` of
    /// its shared cores, or `None` when it reported no cores.
    pub fn build_limit(&self) -> Option<u32> {
        self.build_limit
    }

    fn free_build_slots(&self) -> u32 {
        self.pressure
            .build_slots_free
            .saturating_sub(self.pending_slots)
    }

    /// How many ordinary build handoffs disk and shared cores have room for.
    pub fn free_build_handoffs(&self) -> u32 {
        self.free_build_slots().min(
            self.build_limit
                .map_or(u32::MAX, |limit| limit.saturating_sub(self.heavy)),
        )
    }

    /// All pressure conditions keeping a ticket ready, in admission order.
    pub fn blocked(&self, ticket: &ReadyTicket, machine: &str) -> Option<HandoffWait> {
        if ticket.labels.iter().any(|label| label == JUSTICIAR) {
            // Emergency work can restore space itself.
            return None;
        }
        let mut waits = Vec::new();
        let build = ticket.needs_build_slot;
        if build && self.free_build_slots() == 0 {
            let total = self.pressure.build_slots;
            waits.push(HandoffWait {
                block: Some(StartBlock::Trees),
                reason: format!(
                    "{BUILD_SLOT_WAIT} on {machine}: no slot available to tickets ({total} configured)"
                ),
            });
        }
        if ticket.kind != TicketKind::Regression {
            self.pacing_waits(build, machine, &mut waits);
        }

        let mut waits = waits.into_iter();
        let mut first = waits.next()?;
        first.reason = format!("{} {}", ticket.id, first.reason);
        for wait in waits {
            first.reason.push_str(" · ");
            first.reason.push_str(&wait.reason);
        }
        Some(first)
    }

    fn pacing_waits(&self, build: bool, machine: &str, waits: &mut Vec<HandoffWait>) {
        if self.pressure.queue_backlogged() {
            waits.push(pacing(self.pressure.backlog_reason()));
        }
        if !build {
            return;
        }
        if let Some(limit) = self.build_limit
            && self.heavy >= limit
        {
            waits.push(pacing(format!(
                "{BUILD_LIMIT_WAIT} on {machine}: {} build tickets held, limit {limit}",
                self.heavy
            )));
        }
        if self.pressure.recovery_seconds > 0 {
            let seconds = self.pressure.recovery_seconds;
            waits.push(pacing(format!(
                "{BUILD_QUEUE_RECOVERY_WAIT}: rechecking each round, up to {seconds} s \
                 of recovery left once waits subside"
            )));
        }
        if self.heavy >= INITIAL_BUILD_TICKETS
            && let Some(latest) = self.latest_heavy
        {
            let remaining = f64::from(RAMP_SECONDS) - self.now.seconds_since(latest).max(0.0);
            if remaining > 0.0 {
                waits.push(pacing(format!(
                    "waits for the next build-ticket handoff in {remaining:.0} s"
                )));
            }
        }
    }
}

fn pacing(reason: String) -> HandoffWait {
    HandoffWait {
        reason,
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
    fn a_ticket_that_names_no_files_counts_as_build_work() {
        assert!(likely_compiles(0.0, &[]));
        assert!(!likely_compiles(0.0, &["docs/guide.md".to_owned()]));
        assert!(likely_compiles(0.0, &["src/lib.rs".to_owned()]));
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
