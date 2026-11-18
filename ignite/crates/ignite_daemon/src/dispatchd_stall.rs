//! Workers that went silent: a working session that has run no `ignite` command for 30 minutes
//! is checked through the dispatcher. A running one is left alone, an idle one is asked once to
//! continue or release, and its ticket is released only when the app says the session is gone,
//! or that it is still idle 15 minutes after the reminder. A failed lookup proves nothing.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::DispatchAction;
use ignite_core::time::Timestamp;

use super::{CONTINUE, Dispatcher, IDLE_AFTER_MESSAGE_S, Purpose, SILENT_S, Stall, State};
use crate::server::Context;

impl Dispatcher {
    /// Acts on a `check` answer for `session`.
    pub(super) fn checked(
        &self,
        state: &mut State,
        ctx: &Context,
        session: &SessionId,
        answer: &str,
    ) -> String {
        let now = ctx.clock.now();
        let holding = self.host.holders().get(session).cloned();
        match answer {
            "running" => {
                state.stalls.remove(session);
                format!("; {session} is running")
            }
            "gone" => {
                let _ = self.pool.update(session, |m| m.down = true);
                state.stalls.remove(session);
                state.reserved.remove(session);
                match holding {
                    Some(ticket) => self.release(session, &ticket, "the session is gone"),
                    None => format!("; {session} is down"),
                }
            }
            "idle" => {
                let Some(ticket) = holding else {
                    return format!("; {session} is idle");
                };
                let stall = state.stalls.entry(session.clone()).or_insert(Stall {
                    checked_at: now,
                    messaged_at: None,
                });
                match stall.messaged_at {
                    None => {
                        stall.messaged_at = Some(now);
                        Self::queue(
                            state,
                            Some(session),
                            DispatchAction::Message,
                            CONTINUE,
                            Purpose::Message,
                        );
                        format!("; asked {session} to continue or release")
                    }
                    Some(at) if now.seconds_since(at) >= IDLE_AFTER_MESSAGE_S => {
                        state.stalls.remove(session);
                        self.release(session, &ticket, "the session stayed idle after a reminder")
                    }
                    Some(_) => String::new(),
                }
            }
            // `unknown`, `ok` or `failed`: a lookup that failed proves nothing.
            _ => String::new(),
        }
    }

    fn release(&self, session: &SessionId, ticket: &TicketId, reason: &str) -> String {
        match self.host.release(session, ticket, reason) {
            Ok(()) => format!("; released {ticket} from {session}: {reason}"),
            Err(error) => format!("; could not release {ticket} from {session}: {error}"),
        }
    }

    /// Checks working sessions that have run no command for 30 minutes.
    pub(super) fn watch_silence(&self, state: &mut State, now: Timestamp) {
        let sessions: Vec<SessionId> = state.held.keys().cloned().collect();
        for session in sessions {
            let Some(command) = self.host.command(&session) else {
                continue;
            };
            if command.running {
                state.stalls.remove(&session);
                continue;
            }
            if Self::check_outstanding(state, &session) {
                continue;
            }
            let due = match state.stalls.get(&session) {
                Some(stall) if command.since > stall.checked_at => {
                    state.stalls.remove(&session);
                    now.seconds_since(command.since) >= SILENT_S
                }
                Some(Stall {
                    messaged_at: Some(at),
                    checked_at,
                }) => now.seconds_since(*at) >= IDLE_AFTER_MESSAGE_S && checked_at <= at,
                Some(stall) => now.seconds_since(stall.checked_at) >= SILENT_S,
                None => now.seconds_since(command.since) >= SILENT_S,
            };
            if due {
                let stall = state.stalls.entry(session.clone()).or_insert(Stall {
                    checked_at: now,
                    messaged_at: None,
                });
                stall.checked_at = now;
                Self::queue_check(state, &session);
            }
        }
    }

    fn check_outstanding(state: &State, session: &SessionId) -> bool {
        state
            .pending
            .iter()
            .chain(state.given.values().map(|g| &g.queued))
            .any(|q| q.purpose == Purpose::Check && q.body.session.as_ref() == Some(session))
    }
}
