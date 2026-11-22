//! Active CLI workers as shown in the pool list and hub heartbeat.

use ignite_core::capacity::{CapacityReport, MachineFacts, ProviderCapacity, start_room};
use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::{ProviderOffer, Room};
use ignite_core::protocol::hub::{WorkerRole, WorkerState};
use ignite_core::protocol::machine_state::MachineSample;
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::{HeadlessDispatch, State, cli, shared};
use crate::pool::Member;
use crate::sched::{Scheduler, Status};

impl HeadlessDispatch {
    /// Removes from the pool every chat whose ticket ended: it holds no tree and no reservation,
    /// or the app said it is gone. Its slot is then free. Returns whether any chat left.
    pub(super) fn retire_workers(&self, members: &mut Vec<Member>) {
        let ended: Vec<SessionId> = {
            let state = self.state();
            members
                .iter()
                .filter(|m| cli::is_cli_session(&m.session))
                .filter(|m| {
                    m.down
                        || (!state.held.contains_key(&m.session)
                            && !state.reserved.contains_key(&m.session))
                })
                .map(|m| m.session.clone())
                .collect()
        };
        for session in ended {
            if self.pool.remove(&session).is_ok() {
                self.state().stalls.remove(&session);
                members.retain(|member| member.session != session);
            }
        }
    }

    /// Workers with a session, as shown in the heartbeat and `ignite pool list`.
    pub fn view(&self) -> Result<Vec<WorkerState>> {
        let members = self.pool.members()?;
        let holders = self.host.holders();
        let state = self.state();
        Ok(members
            .iter()
            .filter(|member| cli::is_cli_session(&member.session))
            .filter_map(|member| {
                let held = holders.get(&member.session);
                let reserved = state.reserved.get(&member.session);
                if held.is_none() && reserved.is_none() {
                    return None;
                }
                let status = if member.down {
                    "down"
                } else if held.is_some() {
                    match state.stalls.get(&member.session) {
                        Some(stall) if stall.messaged_at.is_some() => "stalled",
                        _ => "working",
                    }
                } else {
                    "reserved"
                };
                let command = self.host.command(&member.session);
                Some(WorkerState {
                    session: member.session.clone(),
                    app: member.app.clone(),
                    role: WorkerRole::Worker,
                    state: status.to_owned(),
                    ticket: held.or(reserved.map(|r| &r.ticket)).cloned(),
                    files: member.files.clone(),
                    command: command.as_ref().map(|c| c.command.clone()),
                    command_since: command.as_ref().map(|c| c.since),
                    command_running: command.is_some_and(|c| c.running),
                })
            })
            .collect())
    }
}

impl HeadlessDispatch {
    /// Report room before subtracting pending reservations, which the hub already counts.
    pub(super) fn reported_room(
        &self,
        state: &State,
        providers: Vec<ProviderOffer>,
    ) -> (Room, u32, u32) {
        let running = state
            .held
            .keys()
            .filter(|session| cli::is_cli_session(session))
            .count();
        let inputs = self.room_inputs(running, 0);
        let pending = state
            .reserved
            .keys()
            .filter(|session| !state.held.contains_key(*session))
            .count();
        let occupied = inputs
            .running
            .saturating_add(u32::try_from(pending).unwrap_or(u32::MAX));
        let idle = if self.cli.get().is_some() {
            inputs.ceiling.saturating_sub(occupied)
        } else {
            0
        };
        (
            Room {
                start: start_room(&inputs),
                providers,
                memory_pressed: inputs.memory_pressed,
            },
            idle,
            inputs.running,
        )
    }

    pub(crate) fn machine_sample(
        &self,
        now: Timestamp,
        scheduler: &Scheduler,
        facts: &MachineFacts,
    ) -> (MachineSample, CapacityReport) {
        let pressure = self.pressure(now).unwrap_or_default();
        let free_trees = self.host.free_ticket_trees();
        let (cpu_percent, queued_cores, swap_used_mb) = scheduler.machine_readings();
        let active_memory_pressure = scheduler.active_memory_pressure();
        let status = scheduler.status();
        let (room, idle_workers, running_workers, reserved, draining, shared_failure) = {
            let state = self.state();
            let (room, idle_workers, running_workers) =
                self.reported_room(&state, self.provider_offers(&state));
            (
                room,
                idle_workers,
                running_workers,
                u32::try_from(state.reserved.len()).unwrap_or(u32::MAX),
                state.draining,
                state
                    .shared
                    .current
                    .as_ref()
                    .map(shared::SharedFailure::reason),
            )
        };
        let sample = MachineSample {
            pressure,
            reclaimable: None,
            last_disk_cleanup_at: None,
            cpu_percent,
            queued_cores,
            swap_used_mb,
            active_memory_pressure,
            idle_workers,
            room,
            free_ticket_trees: free_trees.saturating_sub(reserved),
            admit_cores: status.cores,
            draining,
            shared_failure,
        };
        let report = capacity_report(
            facts,
            &status,
            &sample,
            running_workers,
            self.host.ticket_work_wait(),
            self.provider_capacity(),
        );
        (sample, report)
    }
}

pub(crate) fn capacity_report(
    facts: &MachineFacts,
    status: &Status,
    sample: &MachineSample,
    running_workers: u32,
    queue_seconds: f64,
    providers: Vec<ProviderCapacity>,
) -> CapacityReport {
    let accepts_work = !sample.draining
        && sample.shared_failure.is_none()
        && sample.free_ticket_trees > 0
        && !sample.pressure.queue_backlogged();
    CapacityReport {
        logical_cores: facts.logical_cores,
        physical_cores: facts.physical_cores,
        memory_mb: facts.memory_mb,
        admit_cores: f64::from(sample.admit_cores),
        free_cores: f64::from(status.free_cores()),
        outside_cores: Some(status.outside),
        running_workers: Some(running_workers),
        worker_room: if accepts_work { sample.room.start } else { 0 },
        free_memory_mb: status.free_memory_mb,
        memory_pressed: status.memory_pressed,
        free_disk_gb: sample.pressure.free_gb,
        disk_budget_gb: 0,
        disk_budget_source: None,
        build_space: None,
        slot_holders: sample.pressure.slot_holders.clone(),
        queue_seconds,
        outputs: sample.pressure.outputs.clone(),
        free_trees: sample.pressure.free_trees.clone(),
        leased_trees: sample.pressure.leased.clone(),
        providers,
    }
}
