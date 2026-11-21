//! Free CLI slots and active workers as shown in the pool list and hub heartbeat.

use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::{WorkerRole, WorkerState};
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::{Dispatcher, State, cli};
use crate::pool::Member;

/// The placeholder of every CLI slot that no CLI session fills, reserved or not, for each app in
/// CLI mode whose slots are offered (see `cli`). The slot for tickets that hold `master` back is
/// offered only while fewer than all of the app's slots are busy.
pub(super) fn cli_placeholders(state: &State, members: &[Member]) -> Vec<Member> {
    let mut slots = Vec::new();
    for layout in state.cli_layout.iter().filter(|layout| layout.offered) {
        let sessions = members
            .iter()
            .filter(|m| !m.down && m.app == layout.app && cli::is_cli_session(&m.session))
            .count();
        let all: Vec<SessionId> = (1..=layout.slots + 1)
            .filter_map(|k| cli::placeholder(&layout.app, k))
            .collect();
        let Some((extra, ordinary)) = all.split_last() else {
            continue;
        };
        let reserved = |s: &SessionId| state.reserved.contains_key(s);
        let busy = sessions + all.iter().filter(|s| reserved(s)).count();
        let free = ordinary.iter().filter(|s| !reserved(s)).skip(sessions);
        let extra_offered = reserved(extra) || busy < all.len();
        let chosen = ordinary
            .iter()
            .filter(|s| reserved(s))
            .chain(free)
            .chain(extra_offered.then_some(extra));
        slots.extend(chosen.map(|session| Member {
            session: session.clone(),
            app: layout.app.clone(),
            down: false,
            models: layout.models.clone(),
            files: Vec::new(),
            added_at: Timestamp::from_millis(0),
            single_use: false,
        }));
    }
    slots
}

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

    /// Every member as the hub's heartbeat and `ignite pool list` show it, and each slot no chat
    /// fills as an idle or reserved worker of its app.
    pub fn view(&self) -> Result<Vec<WorkerState>> {
        self.update_cli_layout();
        let members = self.pool.members()?;
        let holders = self.host.holders();
        let state = self.state();
        let slots = cli_placeholders(&state, &members);
        Ok(members
            .iter()
            .filter(|member| cli::is_cli_session(&member.session))
            .chain(&slots)
            .map(|member| {
                let held = holders.get(&member.session);
                let reserved = state.reserved.get(&member.session);
                let status = if member.down {
                    "down"
                } else if held.is_some() {
                    match state.stalls.get(&member.session) {
                        Some(stall) if stall.messaged_at.is_some() => "stalled",
                        _ => "working",
                    }
                } else if reserved.is_some() {
                    "reserved"
                } else {
                    "idle"
                };
                let command = self.host.command(&member.session);
                WorkerState {
                    session: member.session.clone(),
                    app: member.app.clone(),
                    role: WorkerRole::Worker,
                    state: status.to_owned(),
                    ticket: held.or(reserved.map(|r| &r.ticket)).cloned(),
                    files: member.files.clone(),
                    command: command.as_ref().map(|c| c.command.clone()),
                    command_since: command.as_ref().map(|c| c.since),
                    command_running: command.is_some_and(|c| c.running),
                }
            })
            .collect())
    }
}
