//! The digest of each CLI turn's `output.log`, which the daemon sends to the hub and
//! `ignite pool watch` prints (see "CLI mode" in the design document).
//!
//! - [`digest`] turns the text of one turn's log into lines (what the worker said, each tool call
//!   with the first line of its input, and how the turn ended), each cut to
//!   [`DIGEST_LINE_CHARS`] characters. The reading itself is per app, in
//!   `ignite_core::cli::digest`.
//! - [`CliDaemon::send_digest`] queues the lines of a turn that were not queued yet in the
//!   outbox, which sends them to the hub's `POST /transcripts/append` whenever it is reachable.
//!   What was queued is remembered only in memory. A restarted daemon queues a running turn's
//!   lines again from the start, and the hub stores a line keyed by session, turn and index
//!   once, so nothing is lost and nothing is stored twice.
//! - [`CliDaemon::session_digest`] is the whole digest of a session read from the logs on disk,
//!   for `ignite pool watch`.

use ignite_core::cli::digest as app_digest;
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::cli::{DIGEST_LINE_CHARS, TranscriptAppend, TranscriptLine};
use ignite_core::traits::Result;
use ignite_store::local::cli_slots as store;

use super::{CliDaemon, lock, read};

/// `text` cut to [`DIGEST_LINE_CHARS`] characters, with an ellipsis where it was cut.
fn cut(text: &str) -> String {
    if text.chars().count() <= DIGEST_LINE_CHARS {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(DIGEST_LINE_CHARS - 1).collect();
    kept.push('…');
    kept
}

/// The digest of turn `turn` of `session`, from the text of its `output.log` as `app` prints it.
/// Only whole lines of `output` count, so a line the CLI is still writing is read next time.
pub fn digest(
    app: &str,
    output: &str,
    session: &SessionId,
    ticket: Option<&TicketId>,
    turn: u32,
) -> Vec<TranscriptLine> {
    app_digest::of(app, read::whole_lines(output))
        .into_iter()
        .zip(0..)
        .map(|((kind, text), index)| TranscriptLine {
            session: session.clone(),
            ticket: ticket.cloned(),
            turn,
            index,
            kind,
            text: cut(&text),
            at: None,
        })
        .collect()
}

impl CliDaemon {
    /// Queues the lines of the turn's digest that were not queued before, in the outbox. A
    /// daemon with no machine to name (see [`CliDaemon::with_machine`]) queues nothing. A
    /// failure to queue is reported and the lines are tried again at the next look.
    pub(super) fn send_digest(
        &self,
        app: &str,
        session: &SessionId,
        ticket: Option<&TicketId>,
        turn: u32,
        output: &str,
    ) {
        let Some(machine) = self.machine.clone() else {
            return;
        };
        let lines = digest(app, output, session, ticket, turn);
        let key = (session.clone(), turn);
        let done = lock(&self.sent).get(&key).copied().unwrap_or(0);
        if lines.len() <= done {
            return;
        }
        let now = self.now();
        let fresh: Vec<TranscriptLine> = lines
            .into_iter()
            .skip(done)
            .map(|line| TranscriptLine {
                at: Some(now),
                ..line
            })
            .collect();
        let count = done + fresh.len();
        let append = TranscriptAppend {
            machine,
            lines: fresh,
        };
        let queued = self
            .database(|conn| Ok(crate::outbox::push_transcript(conn, &append, now)))
            .and_then(|result| result);
        match queued {
            Ok(()) => {
                lock(&self.sent).insert(key, count);
            }
            Err(error) => eprintln!("ignite: could not queue the digest of {session}: {error}"),
        }
    }

    /// The digest of every turn of the session that `target` names (its ID, or the ticket it
    /// holds or last held), read from the turns' logs. `None` when no slot has the session.
    pub fn session_digest(&self, target: &str) -> Result<Option<(SessionId, Vec<TranscriptLine>)>> {
        let rows = self.rows()?;
        let Some(row) = rows
            .iter()
            .find(|row| row.session.as_deref() == Some(target))
            .or_else(|| {
                rows.iter()
                    .find(|row| row.ticket.as_deref() == Some(target) && row.session.is_some())
            })
        else {
            return Ok(None);
        };
        let Some(session) = row.session.as_deref().and_then(|s| SessionId::new(s).ok()) else {
            return Ok(None);
        };
        let turns = self.database(|conn| store::turns(conn, session.as_str()))?;
        let mut lines = Vec::new();
        for turn in turns {
            let number = u32::try_from(turn.turn).unwrap_or(0);
            let text = self.turns.output(&session, number).unwrap_or_default();
            let ticket = turn.ticket.as_deref().and_then(|t| TicketId::new(t).ok());
            lines.extend(digest(&turn.app, &text, &session, ticket.as_ref(), number));
        }
        Ok(Some((session, lines)))
    }
}
