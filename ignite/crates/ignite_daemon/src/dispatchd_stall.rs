//! Workers that went silent: a working session that has run no `ignite` command for 30 minutes
//! is checked through the dispatcher. A running one is left alone, an idle one is asked once to
//! continue or release, and its ticket is released only when the app says the session is gone,
//! or that it is still idle 15 minutes after the reminder. A failed lookup proves nothing.
//!
//! A command stuck far past its time counts as a stall too, although it is running: a verb other
//! than `wait`, `land`, `release` or `close` that has run for 45 minutes, or whose current step
//! has lasted 30. A worker once sat in `ignite wait` on an `ignite test` that spent 55 minutes
//! in one step, and was never silent, so nothing looked at it for hours. Such a worker is
//! checked the same way, and an idle one gets a message that names the command, its step and
//! how long each has run, and says what to do with a ticket that cannot go on. A session with a
//! `land`, `release` or `close` running is never looked at for a stuck command: a landing's long
//! waits belong to the landing queue and tip verification.
//!
//! A worker that ended its turn waiting on an `ignite` verb is looked at sooner, once the verb
//! has finished (see `wake`), and an idle one is told the verb finished rather than that it has
//! been silent.
//!
//! Every answer leaves a record of when the session was checked, so the next check comes 30
//! minutes later. A `running` answer used to remove the record, and the next step found the same
//! silence and asked again: one worker writing code without running `ignite` was checked about
//! twice a minute. A release that failed did the same, which reminded an archived session every
//! 15 minutes for hours.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::DispatchAction;
use ignite_core::time::Timestamp;

use super::wake::WAKE;
use super::{
    CONTINUE, Dispatcher, IDLE_AFTER_MESSAGE_S, Purpose, SILENT_S, Stall, State, ends_ticket,
};
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
                    stuck: None,
                    woken: false,
                });
                let stuck = stall.stuck.as_ref().map(|stuck| stuck_message(&stuck.what));
                let unprompted = if stall.woken { WAKE } else { CONTINUE };
                match stall.messaged_at {
                    None => {
                        stall.messaged_at = Some(now);
                        let message = stuck.as_deref().unwrap_or(unprompted);
                        Self::queue(
                            state,
                            Some(session),
                            DispatchAction::Message,
                            message,
                            Purpose::Message,
                        );
                        (format!("; asked {session} to continue or release"), None)
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
            Ok(()) => {
                self.note_released(session, ticket);
                format!("; released {ticket} from {session}: {reason}")
            }
            Err(error) => format!("; could not release {ticket} from {session}: {error}"),
        }
    }

    /// Checks working sessions that have run no command for 30 minutes, or whose command is
    /// stuck past its time.
    pub(super) fn watch_silence(&self, state: &mut State, now: Timestamp) {
        Self::forget_settled(state);
        let sessions: Vec<SessionId> = state.held.keys().cloned().collect();
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
                Self::queue_check(state, &session);
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
            Self::queue_check(state, session);
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
