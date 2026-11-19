//! Workers that went silent: a working session that has run no `ignite` command for 30 minutes
//! is checked through the dispatcher. A running one is left alone, an idle one is asked once to
//! continue or release, and its ticket is released only when the app says the session is gone,
//! or that it is still idle 15 minutes after the reminder. A failed lookup proves nothing.
//!
//! Every answer leaves a record of when the session was checked, so the next check comes 30
//! minutes later. A `running` answer used to remove the record, and the next step found the same
//! silence and asked again: one worker writing code without running `ignite` was checked about
//! twice a minute. A release that failed did the same, which reminded an archived session every
//! 15 minutes for hours.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::DispatchAction;
use ignite_core::time::Timestamp;

use super::{CONTINUE, Dispatcher, IDLE_AFTER_MESSAGE_S, Purpose, SILENT_S, Stall, State};
use crate::server::Context;

/// A ticket to give back once the dispatcher's state is unlocked.
pub(super) struct DueRelease {
    session: SessionId,
    ticket: TicketId,
    reason: &'static str,
}

impl DueRelease {
    fn new(session: &SessionId, ticket: TicketId, reason: &'static str) -> Self {
        Self {
            session: session.clone(),
            ticket,
            reason,
        }
    }
}

impl Dispatcher {
    /// Acts on a `check` answer for `session`. Returns what to add to the acknowledgement and,
    /// when the session's ticket is to be given back, the release for the caller to carry out
    /// with the state unlocked (it asks the hub).
    pub(super) fn checked(
        &self,
        state: &mut State,
        ctx: &Context,
        session: &SessionId,
        answer: &str,
    ) -> (String, Option<DueRelease>) {
        let now = ctx.clock.now();
        let holding = self.host.holders().get(session).cloned();
        match answer {
            "running" => {
                state.stalls.insert(
                    session.clone(),
                    Stall {
                        checked_at: now,
                        messaged_at: None,
                    },
                );
                (format!("; {session} is running"), None)
            }
            "gone" => {
                let _ = self.pool.update(session, |m| m.down = true);
                state.stalls.remove(session);
                state.reserved.remove(session);
                match holding {
                    Some(ticket) => (
                        String::new(),
                        Some(DueRelease::new(session, ticket, "the session is gone")),
                    ),
                    None => (format!("; {session} is down"), None),
                }
            }
            "idle" => {
                let Some(ticket) = holding else {
                    return (format!("; {session} is idle"), None);
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
                        (format!("; asked {session} to continue or release"), None)
                    }
                    // The record stays, so that a release that fails is tried again only at the
                    // next check, 30 minutes on. A release that works ends the hold, and with it
                    // the record (see `notice_holders`).
                    Some(at) if now.seconds_since(at) >= IDLE_AFTER_MESSAGE_S => {
                        stall.checked_at = now;
                        let reason = "the session stayed idle after a reminder";
                        (
                            String::new(),
                            Some(DueRelease::new(session, ticket, reason)),
                        )
                    }
                    Some(_) => (String::new(), None),
                }
            }
            // `unknown`, `ok` or `failed`: a lookup that failed proves nothing.
            _ => (String::new(), None),
        }
    }

    /// Gives a ticket back, and says what happened for the acknowledgement's text.
    pub(super) fn release(&self, due: &DueRelease) -> String {
        let (session, ticket, reason) = (&due.session, &due.ticket, due.reason);
        match self.host.release(session, ticket, reason) {
            Ok(()) => format!("; released {ticket} from {session}: {reason}"),
            Err(error) => format!("; could not release {ticket} from {session}: {error}"),
        }
    }

    /// Checks working sessions that have run no command for 30 minutes.
    pub(super) fn watch_silence(&self, state: &mut State, now: Timestamp) {
        let sessions: Vec<SessionId> = state.held.keys().cloned().collect();
        for session in sessions {
            let since = match self.host.command(&session) {
                Some(command) if command.running => {
                    state.stalls.remove(&session);
                    continue;
                }
                Some(command) => command.since,
                // The server keeps commands in memory, so after a restart it knows nothing of a
                // session that has run nothing since. The claims' saved clock still does; without
                // it, a worker that died before the restart was never checked again.
                None => match self.host.quiet_since(&session) {
                    Some(at) => at,
                    None => continue,
                },
            };
            if Self::check_outstanding(state, &session) {
                continue;
            }
            let due = match state.stalls.get(&session) {
                Some(stall) if since > stall.checked_at => {
                    state.stalls.remove(&session);
                    now.seconds_since(since) >= SILENT_S
                }
                // Reminded and not checked since: the check after the reminder is due 15 minutes
                // after it.
                Some(Stall {
                    messaged_at: Some(at),
                    checked_at,
                }) if checked_at <= at => now.seconds_since(*at) >= IDLE_AFTER_MESSAGE_S,
                Some(stall) => now.seconds_since(stall.checked_at) >= SILENT_S,
                None => now.seconds_since(since) >= SILENT_S,
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

    pub(super) fn check_outstanding(state: &State, session: &SessionId) -> bool {
        state
            .pending
            .iter()
            .chain(state.given.values().map(|g| &g.queued))
            .any(|q| q.purpose == Purpose::Check && q.body.session.as_ref() == Some(session))
    }
}
