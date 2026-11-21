//! Wakes CLI workers whose commands finish and guards against claims from released sessions.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::time::Timestamp;

use super::{Dispatcher, Stall, State, cli};
use crate::claim::StartGate;

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
             ticket, do not restart its commands, and end your turn. Ignite starts a new worker for the next ticket."
        ))
    }

    /// Drops release records after a session claims another ticket, and wait records after it finishes.
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
    /// queues a check whose idle answer gets [`WAKE`]. Returns false
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
        if self.running_verb(session).is_some() || Self::check_outstanding(state, session) {
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
        state.cli_calls.push(cli::Call::Check {
            session: session.clone(),
        });
        true
    }
}

impl StartGate for Dispatcher {
    fn refuse_start(&self, session: &SessionId, ticket: Option<&TicketId>) -> Option<String> {
        Dispatcher::refuse_start(self, session, ticket)
    }
}
