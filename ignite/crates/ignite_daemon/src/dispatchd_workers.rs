//! Active CLI workers as shown in the pool list and hub heartbeat.

use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::{WorkerRole, WorkerState};
use ignite_core::traits::Result;

use super::{Dispatcher, cli};
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
