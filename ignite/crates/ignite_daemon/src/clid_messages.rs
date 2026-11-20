//! Messages for CLI sessions (see "CLI mode" in the design document). An `ignite tell`, a stall
//! reminder or a wake-up for a CLI session is a row of `cli_messages`, never a dispatcher
//! instruction, and the next turn's prompt is every message that no turn has carried yet, in the
//! order they were queued ([`prompt`]).
//!
//! - [`CliDaemon::queue_message`] queues one. A session inside `ignite wait` has the wait ended
//!   with a reply that says a message is waiting, as a desktop session's is: the reply is tool
//!   output, which agents treat as data, so it does not carry the text.
//! - An urgent message (`ignite tell --now`, or any `tell` from the session that holds a
//!   justiciar ticket) also ends the running turn's process group, so the next turn starts with
//!   it at once. While a `land` of the session is under way, ending the turn could leave the
//!   landing half done, so the message waits for the turn's own end.
//! - The turn that carries a message is recorded with `cli_slots::record_turn_start`, which
//!   marks the message delivered in the same transaction. A turn that printed no line never
//!   showed its prompt to the worker, so its messages are put back when it ends (see `end`).

use ignite_core::cli::SlotState;
use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::traits::{Error, Result};
use ignite_store::local::cli_slots as store;
use ignite_store::local::rows::CliMessage;

use super::{CliDaemon, slot_of};

/// The prompt of a turn that delivers `messages`, in order, one paragraph each; `None` for no
/// messages.
pub fn prompt(messages: &[String]) -> Option<String> {
    (!messages.is_empty()).then(|| messages.join("\n\n"))
}

impl CliDaemon {
    /// Queues `text` for `session`'s next turn, then makes sure the session sees it: an urgent
    /// message ends the running turn; otherwise a session inside `ignite wait` has the wait
    /// ended. Fails when the session has no CLI slot.
    pub(super) fn queue_message(
        &self,
        session: &SessionId,
        text: &str,
        urgent: bool,
    ) -> Result<()> {
        let row = self
            .row(session)?
            .ok_or_else(|| Error::NotFound(format!("{session} has no CLI slot")))?;
        let message = CliMessage {
            id: 0,
            session: session.to_string(),
            ticket: row.ticket.clone(),
            text: text.to_owned(),
            urgent: i64::from(urgent),
            queued_at: Some(self.now().to_string()),
            delivered_turn: None,
        };
        self.database(|conn| store::queue_message(conn, &message))?;
        let slot = slot_of(&row);
        if urgent && slot.state == SlotState::Running && !self.host.ending_ticket(session) {
            return self.turns.end(session, slot.turn);
        }
        let notice = match &row.ticket {
            Some(ticket) => format!("{ticket} has a message for you. End your turn to receive it."),
            None => "You have a message. End your turn to receive it.".to_owned(),
        };
        self.host
            .interrupt(session, Reply::text(Exit::Success, notice));
        Ok(())
    }
}
