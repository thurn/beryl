//! A justiciar prompt handed to a worker of this machine without a ticket.
//!
//! The hub files justiciar tickets, so when the hub itself cannot be reached nobody would
//! file one for the outage. The daemon then has its dispatcher hand a justiciar prompt to an
//! idle worker of its own pool directly, through the same hand-off steps as a ticket: clear the
//! session, set the model, send the prompt. That worker is not offered tickets for
//! [`HOLD_S`], the time it may spend on the repair.

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{DispatchAction, DispatchInstruction};
use ignite_core::time::Timestamp;

use super::{Dispatcher, Purpose, Queued, State};
use crate::server::Context;

/// How long a worker given a justiciar prompt is kept from tickets.
pub const HOLD_S: f64 = 2.0 * 3_600.0;

/// Whether `session` is the worker given a justiciar prompt, within its hold.
pub(super) fn holds(state: &State, session: &SessionId, now: Timestamp) -> bool {
    state
        .justiciar
        .as_ref()
        .is_some_and(|(s, at)| s == session && now.seconds_since(*at) < HOLD_S)
}

impl Dispatcher {
    /// Hands `prompt` to an idle worker as a justiciar, unless one is already working on a
    /// justiciar prompt or no worker is idle. Returns the worker given it.
    pub fn hand_off_justiciar(&self, ctx: &Context, prompt: &str) -> Option<SessionId> {
        let now = ctx.clock.now();
        let members = self.pool.members().unwrap_or_default();
        let mut state = self.state();
        if state.down {
            return None;
        }
        if let Some((_, at)) = &state.justiciar
            && now.seconds_since(*at) < HOLD_S
        {
            return None;
        }
        let session = Self::idle_workers(&state, &members, now)
            .into_iter()
            .find(|m| m.models.iter().any(|model| model == "opus"))?
            .session
            .clone();
        let id = Self::next_id(&mut state);
        state.pending.push_back(Queued {
            body: DispatchInstruction {
                id,
                action: DispatchAction::Handoff,
                session: Some(session.clone()),
                clear: true,
                model: Some("opus".to_owned()),
                effort: Some("high".to_owned()),
                prompt: Some(prompt.to_owned()),
            },
            purpose: Purpose::Message,
            gives: 0,
        });
        state.justiciar = Some((session.clone(), now));
        Some(session)
    }
}
