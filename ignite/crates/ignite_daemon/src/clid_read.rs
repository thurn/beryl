//! Reading a running turn: its `output.log` is read from the start whenever the loop looks,
//! and what each line causes is applied once.
//!
//! - Only whole lines count. A line the CLI is still writing has no newline yet and is read
//!   the next time, so a half-written JSON object is never skipped as "not JSON".
//! - `cli_turns.lines_applied` is how many lines' effects were applied. The effects of the
//!   lines after it are the difference between what the whole text says and what its first
//!   `lines_applied` lines say, so reading the same log twice changes nothing.
//! - The effects are: the transcript ID, model and CLI version on the slot and turn rows, new
//!   usage samples for [`super::Effects::usage`], and the model check.
//! - Model check: a Claude turn's first line names the model before any request is made. One
//!   other than the slot asked for (leaving aside a `[1m]` suffix, the long-context variant)
//!   ends the turn, releases the ticket and hides that model for 10 minutes. `--fallback-model`
//!   is never passed, so a different model means the CLI chose it on its own.
//! - A hung turn is one whose `output.log` has not grown while no `ignite` verb of the session
//!   runs: for [`HANG_S`] when the CLI program has no live child process, and for
//!   [`HANG_ANYWAY_S`] whatever its children are or when they cannot be listed. Its process is
//!   ended and its ticket released, as for a worker that stayed idle after a reminder. The
//!   longer limit exists because a CLI can block with its helper processes alive: an `agy`
//!   turn whose provider refused requests for quota wrote nothing for eight hours while its
//!   process lived. A provider's CLI hold does not excuse the silence, and the release frees
//!   the slot instead of parking it with the ticket, so a hold never keeps a hung turn's ticket.

use std::fs;
use std::path::Path;

use ignite_core::cli::{CliApp, SlotState, TurnEnd, TurnEvents};
use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::ReleaseCause;
use ignite_core::traits::{Error, Result};
use ignite_store::local::cli_slots as store;
use ignite_store::local::rows::{CliSlot, CliTurn};

use super::{CliDaemon, Recovery, slot_of};

/// Seconds without growth of `output.log` after which a turn with nothing else running is hung.
pub const HANG_S: f64 = 1800.0;
/// Seconds without growth of `output.log` after which a turn with no `ignite` verb running is
/// hung even if the CLI program has live children. A foreground tool call is cut off by the CLI
/// long before this, so a child silent this long is not doing the worker's work.
pub const HANG_ANYWAY_S: f64 = 3600.0;

/// `text` cut after its last newline: the lines the CLI has finished writing.
pub fn whole_lines(text: &str) -> &str {
    text.rfind('\n').map_or("", |end| &text[..=end])
}

/// The first `lines` lines of `text`, each with its newline.
fn first_lines(text: &str, lines: usize) -> &str {
    if lines == 0 {
        return "";
    }
    text.match_indices('\n')
        .nth(lines - 1)
        .map_or(text, |(end, _)| &text[..=end])
}

/// What the whole lines of `text`, a turn's `output.log` as far as it has been written, say.
pub fn read_text(app: &dyn CliApp, text: &str) -> TurnEvents {
    app.read(whole_lines(text))
}

/// What a turn's `output.log` at `output` says so far, read from its start through `app`. A
/// file that cannot be read says nothing yet.
pub fn read_turn(app: &dyn CliApp, output: &Path) -> TurnEvents {
    let text = fs::read_to_string(output).unwrap_or_default();
    read_text(app, &text)
}

/// A model ID without the `[1m]` suffix Claude adds for its long-context variant.
fn base_model(model: &str) -> &str {
    model.strip_suffix("[1m]").unwrap_or(model)
}

/// Whether a turn asked for `asked` and its first line named `started`.
pub fn wrong_model(asked: Option<&str>, started: Option<&str>) -> bool {
    match (asked, started) {
        (Some(asked), Some(started)) => base_model(asked) != base_model(started),
        _ => false,
    }
}

impl CliDaemon {
    /// Reads the slot's running turn, applies its new lines, and handles its end when its run
    /// says it ended, it was interrupted, or it hung.
    pub(super) fn watch(&self, row: &CliSlot) -> Result<()> {
        let slot = slot_of(row);
        let Some(session) = slot.session.clone() else {
            return Ok(());
        };
        let Some(turn) = self.turn_row(&session, slot.turn)? else {
            return Err(Error::NotFound(format!("turn {} of {session}", slot.turn)));
        };
        if turn.state == "ended" {
            return Ok(());
        }
        let app = self.app(&row.app)?.app.clone();
        // The run is asked first, so the text read after it holds every line of an ended turn.
        let recovery = self.turns.recover(&session, slot.turn);
        let text = self.turns.output(&session, slot.turn).unwrap_or_default();
        let events = read_text(app.as_ref(), &text);
        self.send_digest(&row.app, &session, slot.ticket.as_ref(), slot.turn, &text);
        let mut row = row.clone();
        let turn = self.apply_lines(&mut row, turn, app.as_ref(), &text, &events)?;
        let row = &row;
        if wrong_model(row.model.as_deref(), events.model.as_deref()) {
            return self.refuse_model(row, &turn, &events);
        }
        match recovery {
            Recovery::Running => {
                if let Some(quiet_s) = self.hung(&session, slot.turn) {
                    self.release_hung(row, &turn, &events, quiet_s)
                } else {
                    Ok(())
                }
            }
            Recovery::Ended { exit } => self.finish(row, &turn, &events, exit, None),
            // A run that is gone, or whose supervisor died without a result, was interrupted.
            Recovery::Interrupted | Recovery::NotStarted => {
                self.finish(row, &turn, &events, None, Some(TurnEnd::Interrupted))
            }
        }
    }

    /// Applies the effects of the lines after `turn.lines_applied` and records them as applied.
    fn apply_lines(
        &self,
        row: &mut CliSlot,
        mut turn: CliTurn,
        app: &dyn CliApp,
        text: &str,
        events: &TurnEvents,
    ) -> Result<CliTurn> {
        let applied = u32::try_from(turn.lines_applied).unwrap_or(0);
        if events.lines <= applied {
            return Ok(turn);
        }
        let before = app.read(first_lines(
            whole_lines(text),
            usize::try_from(applied).unwrap_or(0),
        ));
        let fresh = events.usage.get(before.usage.len()..).unwrap_or_default();
        if !fresh.is_empty() {
            self.effects.usage(&slot_of(row), fresh);
        }
        turn.lines_applied = i64::from(events.lines);
        turn.transcript = events.transcript.clone().or(turn.transcript);
        turn.model = events.model.clone().or(turn.model);
        turn.cli_version = events.cli_version.clone().or(turn.cli_version);
        turn.cost_usd = events.cost_usd.or(turn.cost_usd);
        if let Some(transcript) = &events.transcript {
            row.transcript = Some(transcript.clone());
        }
        let session = turn.session.clone();
        let number = turn.turn;
        self.database(|conn| {
            let transaction = conn.transaction()?;
            store::update_turn(&transaction, &turn)?;
            transaction.execute(
                "UPDATE cli_slots SET transcript = ?3 WHERE session = ?1 AND turn = ?2",
                rusqlite::params![session, number, row.transcript],
            )?;
            transaction.commit()?;
            Ok(())
        })?;
        Ok(turn)
    }

    /// How many seconds the session's running turn has been silent, if that makes it hung: its
    /// log has not grown for [`HANG_S`] and the CLI program has no live child, or for
    /// [`HANG_ANYWAY_S`] whatever its children are. A running `ignite` verb of the session, or
    /// a command that ends its ticket, keeps the turn alive at any age.
    fn hung(&self, session: &SessionId, turn: u32) -> Option<f64> {
        let quiet_s = self.now().seconds_since(self.turns.grew_at(session, turn)?);
        if quiet_s < HANG_S
            || !self.verbs_of(session).is_empty()
            || self.host.ending_ticket(session)
        {
            return None;
        }
        if quiet_s >= HANG_ANYWAY_S {
            return Some(quiet_s);
        }
        // A process list that cannot be read counts as a live child until the longer limit.
        self.turns
            .process(session, turn)
            .and_then(|pid| self.host.running_under(pid, None))
            .is_some_and(|children| children.is_empty())
            .then_some(quiet_s)
    }

    /// Ends a hung turn's process, releases its ticket and frees the slot.
    fn release_hung(
        &self,
        row: &CliSlot,
        turn: &CliTurn,
        events: &TurnEvents,
        quiet_s: f64,
    ) -> Result<()> {
        let slot = slot_of(row);
        let Some(session) = slot.session.clone() else {
            return Ok(());
        };
        self.turns.end(&session, slot.turn)?;
        let note = format!(
            "The worker's {} turn wrote nothing for {} minutes with no `ignite` command \
             running, so the turn was ended.",
            row.app,
            (quiet_s / 60.0).round()
        );
        self.effects.ended(&slot, &TurnEnd::Interrupted, events);
        if let Some(ticket) = &slot.ticket
            && self.host.holders().get(&session) == Some(ticket)
        {
            self.host
                .release(&session, ticket, &note, ReleaseCause::TakenBack)?;
        }
        self.close(
            row,
            turn,
            &TurnEnd::Interrupted,
            None,
            SlotState::Free,
            |_| {},
        )
        .map(|_| ())
    }

    /// Ends a turn that started another model than the slot asked for, releases its ticket
    /// and hides the model for [`super::HIDE_MODEL_S`].
    fn refuse_model(&self, row: &CliSlot, turn: &CliTurn, events: &TurnEvents) -> Result<()> {
        let slot = slot_of(row);
        let Some(session) = slot.session.clone() else {
            return Ok(());
        };
        let asked = row.model.clone().unwrap_or_default();
        let started = events.model.clone().unwrap_or_default();
        self.turns.end(&session, slot.turn)?;
        self.hide(&row.app, &asked);
        let note = format!(
            "The {} CLI started the model {started} when {asked} was asked for, so the turn was \
             ended before its first request.",
            row.app
        );
        if let Some(ticket) = &slot.ticket {
            self.host
                .release(&session, ticket, &note, ReleaseCause::TakenBack)?;
        }
        let end = TurnEnd::Unknown { why: note };
        self.close(row, turn, &end, None, SlotState::Free, |_| {})
            .map(|_| ())
    }
}
