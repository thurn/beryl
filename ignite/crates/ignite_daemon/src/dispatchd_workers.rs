//! Active CLI workers as shown in the pool list and hub heartbeat.

use ignite_core::capacity::start_room;
use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::{ProviderOffer, Room};
use ignite_core::protocol::hub::{WorkerRole, WorkerState};
use ignite_core::protocol::machine_state::MachineSample;
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::{Dispatcher, State, cli, shared};
use crate::pool::Member;

impl Dispatcher {
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

impl Dispatcher {
    /// Report room before subtracting pending reservations, which the hub already counts.
    pub(super) fn reported_room(
        &self,
        state: &State,
        providers: Vec<ProviderOffer>,
    ) -> (Room, u32) {
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
        )
    }

    pub(crate) fn machine_sample(
        &self,
        now: Timestamp,
        scheduler: &crate::sched::Scheduler,
        trees: &crate::trees::Trees,
    ) -> MachineSample {
        let pressure = self.pressure(now).unwrap_or_default();
        let free_trees = self.host.free_ticket_trees();
        let (cpu_percent, queued_cores, swap_used_mb) = scheduler.machine_readings();
        let reclaimable = trees.reclaimable_disk();
        let admit_cores = scheduler.status().cores;
        let state = self.state();
        let (room, idle_workers) = self.reported_room(&state, self.provider_offers(&state));
        let reserved = u32::try_from(state.reserved.len()).unwrap_or(u32::MAX);
        MachineSample {
            pressure,
            reclaimable,
            cpu_percent,
            queued_cores,
            swap_used_mb,
            idle_workers,
            room,
            free_ticket_trees: free_trees.saturating_sub(reserved),
            admit_cores,
            draining: state.draining,
            shared_failure: state
                .shared
                .current
                .as_ref()
                .map(shared::SharedFailure::reason),
        }
    }
}
