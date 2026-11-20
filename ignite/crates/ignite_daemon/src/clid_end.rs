//! What follows a turn's end: the facts around it are gathered, `decide` chooses an
//! [`Action`], and the action is carried out.
//!
//! The end is applied once. The turn's row is marked `ended` in the same transaction that
//! gives the slot its next state, and a turn already marked `ended` is never looked at again.
//! Releasing a ticket happens before that transaction: if the daemon dies between the two, the
//! next look finds the turn still unended, the ticket no longer held, and frees the slot.
//! Resuming records the slot as backing off with no pause and the next prompt, then starts the
//! next turn at once, so a daemon that dies between the two starts it on its next look.
//!
//! What each action does to the slot:
//!
//! - `Free`: the slot is free and its session leaves the pool. The ticket and transcript stay
//!   on the row, so the next ticket of the same `lane:` chain can resume the transcript, and
//!   go into `cli_landed`, so a regression the ticket caused can resume it (see `claim`).
//! - `Release`: the ticket is given back through [`crate::dispatchd::Host::release`] with the
//!   note, then as `Free`.
//! - `Resume`: the next turn starts now with the prompt.
//! - `WaitForVerb`: the slot waits until no `ignite` verb of the session runs.
//! - `BackOff`: the slot starts its next turn at the time given.
//! - `Hold`: the app goes on a CLI hold through [`super::Effects::hold`] and the slot is parked
//!   with its ticket held.
//! - `Park`: the slot is parked with its ticket held, and the report goes to
//!   [`super::Effects::report`].
//!
//! One end skips `decide`: the first turn of a slot that resumed a culprit's transcript, if it
//! ends as anything but `Clean`. The resume failed (the transcript is gone, or the CLI refused
//! it), so the slot starts the ticket again at once on a new transcript with its fallback
//! prompt, and no hold, park or release follows from that end.

use ignite_core::cli::{Action, Facts, PreviousAnswer, SlotState, TurnEnd, TurnEvents};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::cli_slots as store;
use ignite_store::local::rows::{CliSlot, CliTurn};

use super::{CliDaemon, CliPool, slot_of};

impl CliDaemon {
    /// Handles the end of the slot's turn: `forced` is the end when the daemon knows it (an
    /// interrupted or hung turn); otherwise the app classifies the events and `exit`.
    pub(super) fn finish(
        &self,
        row: &CliSlot,
        turn: &CliTurn,
        events: &TurnEvents,
        exit: Option<i32>,
        forced: Option<TurnEnd>,
    ) -> Result<()> {
        let app = self.app(&row.app)?.app.clone();
        let end = forced.unwrap_or_else(|| app.classify(events, exit));
        if let Some(fallback) = &row.fallback_prompt
            && end != TurnEnd::Clean
        {
            self.effects.ended(&slot_of(row), &end, events);
            let now = self.now().to_string();
            let next = self.close(row, turn, &end, exit, SlotState::BackingOff, |r| {
                r.transcript = None;
                r.next_prompt = Some(fallback.clone());
                r.resume_at = Some(now);
            })?;
            return self.start_next(&next);
        }
        let facts = self.facts(row, turn, &end, events)?;
        let action = (self.rules.decide)(&slot_of(row), &end, &facts);
        self.effects.ended(&slot_of(row), &end, events);
        self.apply(row, turn, &end, exit, &facts, &action, events)
    }

    /// What `decide` needs besides the slot and the end.
    fn facts(
        &self,
        row: &CliSlot,
        turn: &CliTurn,
        end: &TurnEnd,
        events: &TurnEvents,
    ) -> Result<Facts> {
        let slot = slot_of(row);
        let session = slot
            .session
            .clone()
            .ok_or_else(|| Error::Invalid(format!("slot {} has no session", row.slot)))?;
        let ticket_held = slot
            .ticket
            .as_ref()
            .is_some_and(|ticket| self.host.holders().get(&session) == Some(ticket));
        let waiting = self.database(|conn| store::undelivered(conn, session.as_str()))?;
        // `ignite tell --now` ends the running turn on purpose, so that end says nothing about
        // whether the worker is stuck.
        let cut_for_message = waiting.iter().any(|m| m.urgent != 0);
        let messages = waiting.into_iter().map(|m| m.text).collect();
        let interrupted_in_row = if *end == TurnEnd::Interrupted {
            // An `ignite` command run during the turn shows the worker was making progress,
            // so the count starts again with this end.
            let started = turn
                .started_at
                .as_deref()
                .and_then(|t| Timestamp::parse(t).ok());
            let ran_since = match (self.host.quiet_since(&session), started) {
                (Some(quiet), Some(started)) => quiet > started,
                _ => false,
            };
            if ran_since || cut_for_message {
                1
            } else {
                u32::try_from(row.interrupted_in_row).unwrap_or(0) + 1
            }
        } else {
            0
        };
        let previous = if row.block_sent != 0 {
            PreviousAnswer::BlockSent
        } else if row.backed_off != 0 {
            PreviousAnswer::BackedOff
        } else {
            PreviousAnswer::Other
        };
        Ok(Facts {
            now: self.now(),
            ticket_held,
            running_verbs: self.verbs_of(&session),
            land_pushing: self.host.ending_ticket(&session),
            messages,
            interrupted_in_row,
            previous,
            hold: self.held(&row.app),
            last_text: events.last_text.clone(),
        })
    }

    /// Carries out what `decide` chose after the slot's turn ended.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply(
        &self,
        row: &CliSlot,
        turn: &CliTurn,
        end: &TurnEnd,
        exit: Option<i32>,
        facts: &Facts,
        action: &Action,
        events: &TurnEvents,
    ) -> Result<()> {
        let slot = slot_of(row);
        let interrupted = if *end == TurnEnd::Interrupted {
            i64::from(facts.interrupted_in_row)
        } else {
            0
        };
        let block_sent = *end == TurnEnd::Clean
            && matches!(action, Action::Resume { .. })
            && facts.messages.is_empty()
            && facts.running_verbs.is_empty();
        let backed_off = matches!(action, Action::BackOff { .. });
        let counters = |r: &mut CliSlot| {
            r.interrupted_in_row = interrupted;
            r.block_sent = i64::from(block_sent);
            r.backed_off = i64::from(backed_off);
        };
        match action {
            Action::Free => {
                self.record_landed(row)?;
                self.close(row, turn, end, exit, SlotState::Free, |_| {})?;
            }
            Action::Release { note } => {
                if let (Some(session), Some(ticket)) = (&slot.session, &slot.ticket) {
                    self.host.release(session, ticket, note)?;
                }
                self.close(row, turn, end, exit, SlotState::Free, |_| {})?;
            }
            Action::Resume { prompt } => {
                let now = self.now().to_string();
                let next = self.close(row, turn, end, exit, SlotState::BackingOff, |r| {
                    counters(r);
                    r.next_prompt = Some(prompt.clone());
                    r.resume_at = Some(now);
                })?;
                self.start_next(&next)?;
            }
            Action::WaitForVerb { handle, prompt } => {
                self.close(row, turn, end, exit, SlotState::WaitingVerb, |r| {
                    counters(r);
                    r.next_prompt = Some(prompt.clone());
                    r.wait_handle = Some(handle.clone());
                })?;
            }
            Action::BackOff { at, prompt } => {
                self.close(row, turn, end, exit, SlotState::BackingOff, |r| {
                    counters(r);
                    r.next_prompt = Some(prompt.clone());
                    r.resume_at = Some(at.to_string());
                })?;
            }
            Action::Hold { reason, message } => {
                self.effects
                    .hold(&row.app, *reason, message, events.cli_version.as_deref());
                self.close(row, turn, end, exit, SlotState::Parked, |r| {
                    counters(r);
                    r.park_reason = Some(format!("CLI hold ({}): {message}", reason.as_str()));
                })?;
            }
            Action::Park { report } => {
                self.effects.report(&slot, report);
                self.close(row, turn, end, exit, SlotState::Parked, |r| {
                    counters(r);
                    r.park_reason = Some(report.clone());
                })?;
            }
        }
        Ok(())
    }

    /// Marks the turn ended as `end` and gives the slot `state`, changed by `change`, in one
    /// transaction. A slot made free also loses its session, which leaves the pool. Returns
    /// the slot's new row.
    pub(super) fn close(
        &self,
        row: &CliSlot,
        turn: &CliTurn,
        end: &TurnEnd,
        exit: Option<i32>,
        state: SlotState,
        change: impl FnOnce(&mut CliSlot),
    ) -> Result<CliSlot> {
        let now = self.now().to_string();
        let mut turn = turn.clone();
        "ended".clone_into(&mut turn.state);
        turn.ended_at = Some(now.clone());
        turn.exit = exit.map(i64::from);
        turn.end_kind = serde_json::to_string(end).ok();
        let mut next = row.clone();
        state.as_str().clone_into(&mut next.state);
        next.since = Some(now);
        next.next_prompt = None;
        next.wait_handle = None;
        next.resume_at = None;
        next.park_reason = None;
        // Only a slot's first turn after a resume falls back to a new transcript.
        next.fallback_prompt = None;
        if state == SlotState::Free {
            freed(&mut next);
        }
        change(&mut next);
        // A turn that printed no line never showed its prompt to the worker, so the messages
        // it carried wait for the next turn again. A slot made free has no next turn.
        let requeue = turn.lines_applied == 0 && state != SlotState::Free;
        self.database(|conn| {
            let transaction = conn.transaction()?;
            store::update_turn(&transaction, &turn)?;
            if requeue {
                store::requeue_messages(&transaction, &turn.session, turn.turn)?;
            }
            // The slot's row is keyed by app and number, so it is written only while it still
            // holds this session.
            let current = store::by_session(&transaction, &turn.session)?;
            if current.is_some() {
                store::put(&transaction, &next)?;
            }
            transaction.commit()?;
            Ok(())
        })?;
        if state == SlotState::Free
            && let Some(session) = &slot_of(row).session
        {
            self.pool.remove(session)?;
        }
        Ok(next)
    }

    /// Frees a parked slot whose session no longer holds its ticket: someone released it or
    /// its lease expired. Nothing else looks at such a slot, since only a CLI hold's end
    /// resumes parked slots, so a slot parked for an end `decide` could not place would
    /// otherwise stay taken for good.
    ///
    /// Left alone: a slot a CLI hold parked, which the hold's end resumes (its first turn may
    /// not have run `ignite start` yet, so its session holds no ticket tree, and freeing it
    /// would drop the hand-off); a slot with no ticket (a justiciar), left for a person; and a
    /// state this build does not know, which only reads as parked. The row is written only if
    /// it is still parked, and the session leaves the pool only if it was: the health loop
    /// can resume the slot meanwhile.
    pub(super) fn free_if_released(&self, row: &CliSlot) -> Result<()> {
        if row.state != SlotState::Parked.as_str()
            || super::health::parked_by_hold(row.park_reason.as_deref())
        {
            return Ok(());
        }
        let slot = slot_of(row);
        let (Some(session), Some(ticket)) = (&slot.session, &slot.ticket) else {
            return Ok(());
        };
        if self.host.holders().get(session) == Some(ticket) {
            return Ok(());
        }
        let mut next = row.clone();
        SlotState::Free.as_str().clone_into(&mut next.state);
        next.since = Some(self.now().to_string());
        next.park_reason = None;
        next.next_prompt = None;
        next.fallback_prompt = None;
        freed(&mut next);
        let written = self.database(|conn| {
            let transaction = conn.transaction()?;
            let current = store::by_session(&transaction, session.as_str())?;
            let parked = current.is_some_and(|c| c.state == SlotState::Parked.as_str());
            if parked {
                store::put(&transaction, &next)?;
            }
            transaction.commit()?;
            Ok(parked)
        })?;
        if written {
            self.pool.remove(session)?;
        }
        Ok(())
    }
}

/// Clears what a slot made free keeps of its session. The ticket and transcript stay, for the
/// next ticket of a `lane:` chain.
fn freed(row: &mut CliSlot) {
    row.session = None;
    row.reservation = None;
    row.interrupted_in_row = 0;
    row.block_sent = 0;
    row.backed_off = 0;
}
