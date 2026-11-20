//! Workers that end their turn to wait on a long `ignite` command, and workers whose ticket
//! Ignite took back.
//!
//! Waiting. A worker that starts `ignite test`, `ignite check`, `ignite run -- <command>` or
//! `ignite wait` in the background and ends its turn is waiting, not abandoning its ticket, so
//! the `Stop` hook (see `turn`) lets it stop. Its tool normally wakes it when the command exits,
//! but that is not guaranteed: a worker once ended many turns with a background `test` running
//! and nothing woke it. So the daemon guarantees the wake-up. The turn end is recorded, and once
//! nothing the session started is running any more, and [`WAKE_S`] has passed since the last one
//! ended, the session is checked through the dispatcher as a silent one would be (see `stall`).
//! A CLI session is checked as soon as its verb ends, without the wait: it has no tool of its
//! own that could have woken it, and its check is answered by the CLI pool (see `cli`).
//! A worker that its tool already woke answers `running` and nothing more happens; an idle one
//! gets [`WAKE`], and is released if it stays idle 15 minutes after that, as after any reminder.
//! A background shell command is not an `ignite` verb: the turn-end check finds it among the
//! live processes under the session's own process instead, and leaves its wake-up to the agent
//! app, with the stall rule behind it.
//!
//! Released. When Ignite releases a worker's ticket itself (its second stop, its ⛔ title, or the
//! stall rule), the session may still be alive: a background task it started can report back and
//! wake it. Workers woken that way once claimed the same ticket again, or restarted its commands,
//! while the hub had already reserved it for another worker, so each release added one more
//! session working the ticket. So the dispatcher remembers the session ([`State::released`]) and
//! `ignite start` refuses it (see [`crate::claim::StartGate`]) until the dispatcher hands it a
//! ticket: it may then start only the ticket reserved for it. A hand-off also clears the session
//! first, which ends whatever it was still doing. The record ends when the session holds a ticket
//! again or leaves the pool, and it is saved, so a restarted daemon still refuses.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::time::Timestamp;

use super::{Dispatcher, Stall, State};
use crate::claim::StartGate;

/// Seconds after the last command a waiting session started has ended before the session is
/// checked. Its tool usually wakes it at once, so by then a woken worker is seen running.
pub const WAKE_S: f64 = 120.0;

/// The message an idle worker gets when the command it ended its turn waiting on has finished.
pub const WAKE: &str = "The `ignite` command you ended your turn waiting on has finished. Read its \
                        result with `ignite wait <handle>` or `ignite status`, then continue the \
                        ticket, or run `ignite release` with its exact state.";

impl Dispatcher {
    /// The first `ignite` verb `session` started that is still running, such as `test`, or
    /// `None` when nothing it started runs.
    pub(super) fn running_verb(&self, session: &SessionId) -> Option<String> {
        if let Some(verb) = self.host.running_verbs(session).into_iter().next() {
            return Some(verb.verb);
        }
        self.host
            .command(session)
            .filter(|command| command.running)
            .map(|command| {
                let verb = command.command.split_whitespace().next().unwrap_or("");
                verb.to_owned()
            })
    }

    /// Records that `session` ended its turn at `now` waiting on a running verb, or, with
    /// `waiting` false, that it ended one without.
    pub(super) fn note_turn_end(&self, session: &SessionId, waiting: bool, now: Timestamp) {
        let mut state = self.state();
        if waiting {
            state.waiting.insert(session.clone(), now);
        } else {
            state.waiting.remove(session);
        }
        drop(state);
        self.save();
    }

    /// Records that Ignite released `ticket` from `session`.
    pub(super) fn note_released(&self, session: &SessionId, ticket: &TicketId) {
        self.state()
            .released
            .insert(session.clone(), ticket.clone());
        self.save();
    }

    /// Why `session` may not run `ignite start` for `ticket` (`None` for a summary), or `None`
    /// when it may.
    pub fn refuse_start(&self, session: &SessionId, ticket: Option<&TicketId>) -> Option<String> {
        let state = self.state();
        let released = state.released.get(session)?;
        let reserved = state.reserved.get(session).map(|r| &r.ticket);
        if ticket.is_some() && reserved == ticket {
            return None;
        }
        Some(format!(
            "refused  Ignite released {released} from this session, and its work is saved on the \
             remote for whoever takes it next. Stop now: do not start it or any other \
             ticket, do not restart its commands, and end your turn. The dispatcher hands you \
             your next ticket."
        ))
    }

    /// Ends the records of sessions that hold a ticket again, which only a start the dispatcher
    /// allowed gives them, and the waits of sessions that hold none.
    pub(super) fn forget_settled(state: &mut State) {
        let State {
            released,
            waiting,
            held,
            ..
        } = state;
        released.retain(|session, _| !held.contains_key(session));
        waiting.retain(|session, _| held.contains_key(session));
    }

    /// Looks at `session` if it ended its turn waiting on a verb: once nothing it started runs
    /// and [`WAKE_S`] has passed, queues a check whose idle answer gets [`WAKE`]. Returns false
    /// when the session is not waiting, so the silence rule looks at it instead.
    pub(super) fn watch_waiting(
        &self,
        state: &mut State,
        session: &SessionId,
        now: Timestamp,
    ) -> bool {
        if !state.waiting.contains_key(session) {
            return false;
        }
        if self.running_verb(session).is_some() {
            return true;
        }
        let ended = match self.host.command(session) {
            Some(command) => Some(command.since),
            None => self.host.quiet_since(session),
        };
        // A CLI session's next turn is started by the daemon, not by a tool that may already
        // have woken it, so its wake-up goes out as soon as the verb ends.
        let due = super::cli::is_cli_session(session)
            || ended.is_none_or(|at| now.seconds_since(at) >= WAKE_S);
        if !due || Self::check_outstanding(state, session) {
            return true;
        }
        state.waiting.remove(session);
        state.stalls.insert(
            session.clone(),
            Stall {
                checked_at: now,
                messaged_at: None,
                stuck: None,
                woken: true,
            },
        );
        Self::queue_check(state, session);
        true
    }
}

impl StartGate for Dispatcher {
    fn refuse_start(&self, session: &SessionId, ticket: Option<&TicketId>) -> Option<String> {
        Dispatcher::refuse_start(self, session, ticket)
    }
}
