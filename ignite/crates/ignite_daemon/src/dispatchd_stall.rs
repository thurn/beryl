//! Checks CLI workers that go silent or whose commands stop making progress.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::time::Timestamp;

use super::wake::WAKE;
use super::{CONTINUE, Dispatcher, IDLE_AFTER_MESSAGE_S, SILENT_S, Stall, State, cli, ends_ticket};
use crate::clid::SessionCheck;
use crate::loop_health::span;
use crate::server::Context;

/// Seconds a verb may run before it counts as stuck.
pub const STUCK_COMMAND_S: f64 = 2700.0;
/// Seconds one step of a verb may last before the verb counts as stuck.
pub const STUCK_STEP_S: f64 = 1800.0;

/// A verb a session started that is still running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningVerb {
    /// The verb, such as `test`.
    pub verb: String,
    pub started: Timestamp,
    /// The verb's current step and when it began, if it reported one.
    pub step: Option<(String, Timestamp)>,
}

/// A command stuck past its time, as recorded on a stall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Stuck {
    /// When the command crossed one of the limits. A new stuck command, or a new step that
    /// crosses the step limit later, is a new stall.
    pub(super) at: Timestamp,
    /// What the command is doing and for how long, for the message, as of the last check.
    pub(super) what: String,
}

/// The message an idle worker gets when a command it started is stuck.
fn stuck_message(what: &str) -> String {
    format!(
        "{what}. If the ticket can still be done, continue it. If it waits on something \
         outside the ticket, file that as a ticket, make it a dependency of this one and run \
         `ignite release`. If it cannot be completed as written, run `ignite justiciar \
         \"<what is impossible and the evidence>\"` and then `ignite release`."
    )
}

/// When `verb` crossed a limit and what it is doing, or `None` while it is within both.
fn overdue(verb: &RunningVerb, now: Timestamp) -> Option<Stuck> {
    let by_age = verb.started.plus_seconds(STUCK_COMMAND_S);
    let at = match &verb.step {
        Some((_, began)) => by_age.min(began.plus_seconds(STUCK_STEP_S)),
        None => by_age,
    };
    if now < at {
        return None;
    }
    let mut what = format!(
        "Your `ignite {}` has run for {}",
        verb.verb,
        span(now.seconds_since(verb.started))
    );
    if let Some((step, began)) = &verb.step {
        let _ = write!(
            what,
            ", its current step \"{step}\" for {}",
            span(now.seconds_since(*began))
        );
    }
    Some(Stuck { at, what })
}

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
    /// Handles a CLI session check and returns any release to perform outside the state lock.
    pub(super) fn checked(
        &self,
        state: &mut State,
        ctx: &Context,
        session: &SessionId,
        answer: SessionCheck,
    ) -> Option<DueRelease> {
        let now = ctx.clock.now();
        let holding = self.host.holders().get(session).cloned();
        match answer {
            SessionCheck::Running => {
                // A stuck command's record is kept, so the same stuck command is looked at again
                // only after another 30 minutes.
                let stuck = state.stalls.remove(session).and_then(|stall| stall.stuck);
                state.stalls.insert(
                    session.clone(),
                    Stall {
                        checked_at: now,
                        messaged_at: None,
                        stuck,
                        woken: false,
                    },
                );
                None
            }
            SessionCheck::Gone => {
                let _ = self.pool.update(session, |m| m.down = true);
                state.stalls.remove(session);
                state.reserved.remove(session);
                holding.map(|ticket| DueRelease::new(session, ticket, "the session is gone"))
            }
            SessionCheck::Idle => {
                let ticket = holding?;
                let stall = state.stalls.entry(session.clone()).or_insert(Stall {
                    checked_at: now,
                    messaged_at: None,
                    stuck: None,
                    woken: false,
                });
                let stuck = stall.stuck.as_ref().map(|stuck| stuck_message(&stuck.what));
                let unprompted = if stall.woken { WAKE } else { CONTINUE };
                match stall.messaged_at {
                    None => {
                        stall.messaged_at = Some(now);
                        let message = stuck.as_deref().unwrap_or(unprompted);
                        state.cli_calls.push(cli::Call::Message {
                            session: session.clone(),
                            text: message.to_owned(),
                            urgent: false,
                        });
                        None
                    }
                    // The record stays, so that a release that fails is tried again only at the
                    // next check, 30 minutes on. A release that works ends the hold, and with it
                    // the record (see `notice_holders`).
                    Some(at) if now.seconds_since(at) >= IDLE_AFTER_MESSAGE_S => {
                        stall.checked_at = now;
                        let reason = if stuck.is_some() {
                            "the session stayed idle with a command stuck past its time"
                        } else {
                            "the session stayed idle after a reminder"
                        };
                        Some(DueRelease::new(session, ticket, reason))
                    }
                    Some(_) => None,
                }
            }
        }
    }

    /// Gives a ticket back, and returns the outcome for the daemon log.
    pub(super) fn release(&self, due: &DueRelease) -> String {
        let (session, ticket, reason) = (&due.session, &due.ticket, due.reason);
        match self.host.release(session, ticket, reason) {
            Ok(()) => {
                self.note_released(session, ticket);
                format!("released {ticket} from {session}: {reason}")
            }
            Err(error) => format!("could not release {ticket} from {session}: {error}"),
        }
    }

    /// Checks working sessions that have run no command for 30 minutes, or whose command is
    /// stuck past its time.
    pub(super) fn watch_silence(&self, state: &mut State, now: Timestamp) {
        Self::forget_settled(state);
        let sessions: Vec<SessionId> = state
            .held
            .keys()
            .filter(|session| cli::is_cli_session(session))
            .cloned()
            .collect();
        for session in sessions {
            if let Some(stuck) = self.stuck(&session, now) {
                Self::watch_stuck(state, &session, stuck, now);
                continue;
            }
            if self.watch_waiting(state, &session, now) {
                continue;
            }
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
                    ..
                }) if checked_at <= at => now.seconds_since(*at) >= IDLE_AFTER_MESSAGE_S,
                Some(stall) => now.seconds_since(stall.checked_at) >= SILENT_S,
                None => now.seconds_since(since) >= SILENT_S,
            };
            if due {
                let stall = state.stalls.entry(session.clone()).or_insert(Stall {
                    checked_at: now,
                    messaged_at: None,
                    stuck: None,
                    woken: false,
                });
                stall.checked_at = now;
                stall.stuck = None;
                state.cli_calls.push(cli::Call::Check { session });
            }
        }
    }

    /// The command of `session` that has been stuck longest, or `None` when none is stuck or the
    /// session is ending its ticket.
    fn stuck(&self, session: &SessionId, now: Timestamp) -> Option<Stuck> {
        if self.host.ending_ticket(session) {
            return None;
        }
        self.host
            .running_verbs(session)
            .iter()
            // `wait` only waits on another verb, which is listed itself.
            .filter(|verb| verb.verb != "wait" && !ends_ticket(&verb.verb))
            .filter_map(|verb| overdue(verb, now))
            .min_by_key(|stuck| stuck.at)
    }

    /// Checks a session whose command is stuck: at once for a new stuck command, 15 minutes
    /// after a reminder, and otherwise 30 minutes after the last check.
    fn watch_stuck(state: &mut State, session: &SessionId, stuck: Stuck, now: Timestamp) {
        if Self::check_outstanding(state, session) {
            return;
        }
        let due = match state.stalls.get(session) {
            Some(stall) if stall.stuck.as_ref().is_some_and(|s| s.at == stuck.at) => {
                match stall.messaged_at {
                    Some(at) if stall.checked_at <= at => {
                        now.seconds_since(at) >= IDLE_AFTER_MESSAGE_S
                    }
                    _ => now.seconds_since(stall.checked_at) >= SILENT_S,
                }
            }
            // Silence, or a different stuck command: this one is new.
            _ => {
                state.stalls.remove(session);
                true
            }
        };
        if due {
            let stall = state.stalls.entry(session.clone()).or_insert(Stall {
                checked_at: now,
                messaged_at: None,
                stuck: None,
                woken: false,
            });
            stall.checked_at = now;
            stall.stuck = Some(stuck);
            state.cli_calls.push(cli::Call::Check {
                session: session.clone(),
            });
        }
    }

    pub(super) fn check_outstanding(state: &State, session: &SessionId) -> bool {
        state
            .cli_calls
            .iter()
            .any(|call| matches!(call, cli::Call::Check { session: pending } if pending == session))
    }
}
