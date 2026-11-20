//! Messages for CLI sessions (see "CLI mode" in the design document, "Messages, usage and
//! repair"). An `ignite tell`, a stall reminder or a wake-up for a CLI session is a row of
//! `cli_messages`, never a dispatcher instruction, and the next turn's prompt is every message
//! that no turn has carried yet, in the order they were queued.
//!
//! - [`message`] queues one. A session inside `ignite wait` has the wait ended with a reply
//!   that says a message is waiting, as a desktop session's is: the reply is tool output, which
//!   agents treat as data, so it does not carry the text.
//! - An urgent message (`ignite tell --now`, or any `tell` from the session that holds a
//!   justiciar ticket) also ends the running turn's process group, so the next turn starts with
//!   it at once (see [`press`]). While a `land` of the session has begun its push, ending the
//!   turn would leave the landing half done, so the message waits for the land, and the CLI
//!   loop calls [`press`] again until the turn can be ended.
//! - [`pending`] gathers what the next turn delivers. The caller records the turn with
//!   `cli_slots::record_turn_start`, which marks those messages delivered in the same
//!   transaction. A turn that printed no line never reached the worker, so [`requeue_if_silent`]
//!   puts its messages back.
//! - A turn ended for an urgent message leaves [`CUT_OFF_MARK`] in its run directory, and the
//!   turn that follows starts with [`CUT_OFF_TEXT`] instead of the interrupted text (see
//!   [`resume_prompt`]).

use std::fs;
use std::io;
use std::path::Path;

use ignite_core::cli::decide::INTERRUPTED_TEXT;
use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::cli_slots;
use ignite_store::local::rows::{CliMessage, CliTurn};
use rusqlite::Connection;

use crate::dispatchd::Host;

/// The file left in the run directory of a turn ended to deliver an urgent message.
pub const CUT_OFF_MARK: &str = "cut-off";

/// The first line of the turn that follows one ended to deliver an urgent message.
pub const CUT_OFF_TEXT: &str = "Ignite ended your previous turn to deliver an urgent message, \
                                so your last tool call was cut off and may not have finished. \
                                Run `ignite status` before you continue.";

/// What message delivery needs from the rest of the daemon.
pub trait Deliverer {
    /// Ends the session's `ignite wait` at once with `reply`. Returns whether it was waiting.
    fn interrupt(&self, session: &SessionId, reply: Reply) -> bool;
    /// Whether a `land` of the session runs and has begun its push to the staging branch.
    fn land_pushing(&self, session: &SessionId) -> bool;
    /// Ends the command of the run in directory `run` and every process in its group.
    fn end_run(&self, run: &Path) -> io::Result<()>;
}

/// Whether a `land` of `session` runs and its current step is the push, by the step names
/// `land` reports ("pushing <commit> to <branch>"). Ending the turn or releasing the ticket
/// then would reset the tree under the push.
pub fn land_pushing(host: &dyn Host, session: &SessionId) -> bool {
    host.ending_ticket(session)
        && host.running_verbs(session).iter().any(|verb| {
            verb.verb == "land"
                && verb
                    .step
                    .as_ref()
                    .is_some_and(|(step, _)| step.starts_with("pushing "))
        })
}

/// What [`message`] or [`press`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The message waits for the session's next turn.
    Queued,
    /// The message waits for the next turn, and the session's `ignite wait` was ended so that
    /// it ends its turn.
    WaitEnded,
    /// An urgent message ended the running turn, whose run directory this is.
    TurnEnded(String),
    /// An urgent message waits for a `land` of the session to finish its push before the turn
    /// is ended.
    HeldForPush,
}

/// The messages the next turn of a session delivers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    /// Their rows' IDs, for `cli_slots::record_turn_start`.
    pub ids: Vec<i64>,
    /// Their texts, in the order they were queued.
    pub texts: Vec<String>,
}

/// The prompt of a turn that delivers `messages`, in order, one paragraph each; `None` for no
/// messages.
pub fn prompt(messages: &[String]) -> Option<String> {
    (!messages.is_empty()).then(|| messages.join("\n\n"))
}

/// The prompt of the turn that follows an interrupted one, given the prompt `decide` chose.
/// When the turn was ended for an urgent message (its run directory `run` holds
/// [`CUT_OFF_MARK`]), the interrupted text at its start becomes [`CUT_OFF_TEXT`], which says
/// why. Any other prompt is kept.
pub fn resume_prompt(chosen: &str, run: Option<&Path>) -> String {
    match chosen.strip_prefix(INTERRUPTED_TEXT) {
        Some(rest) if run.is_some_and(was_cut_off) => format!("{CUT_OFF_TEXT}{rest}"),
        _ => chosen.to_owned(),
    }
}

/// Whether the turn whose run directory is `run` was ended for an urgent message.
pub fn was_cut_off(run: &Path) -> bool {
    run.join(CUT_OFF_MARK).exists()
}

/// Every message of `session` that no turn has carried yet, in order.
pub fn pending(conn: &Connection, session: &SessionId) -> Result<Pending> {
    let rows = cli_slots::undelivered(conn, session.as_str()).map_err(store)?;
    Ok(Pending {
        ids: rows.iter().map(|m| m.id).collect(),
        texts: rows.into_iter().map(|m| m.text).collect(),
    })
}

/// Puts back the messages turn `turn` of `session` carried when the turn printed no line
/// (`lines` is the count its reader gave), since its prompt never reached the worker. Returns
/// how many were put back.
pub fn requeue_if_silent(
    conn: &Connection,
    session: &SessionId,
    turn: u32,
    lines: u32,
) -> Result<usize> {
    if lines > 0 {
        return Ok(0);
    }
    cli_slots::requeue_messages(conn, session.as_str(), i64::from(turn)).map_err(store)
}

/// Queues `text` for `session`'s next turn, then makes sure the session sees it: an urgent
/// message ends the running turn (see [`press`]); otherwise a session inside `ignite wait` has
/// the wait ended. Fails when the session has no CLI slot.
pub fn message(
    conn: &Connection,
    deliverer: &dyn Deliverer,
    session: &SessionId,
    text: &str,
    urgent: bool,
    now: Timestamp,
) -> Result<Delivery> {
    let slot = cli_slots::by_session(conn, session.as_str())
        .map_err(store)?
        .ok_or_else(|| Error::Invalid(format!("{session} has no CLI slot")))?;
    cli_slots::queue_message(
        conn,
        &CliMessage {
            id: 0,
            session: session.to_string(),
            ticket: slot.ticket.clone(),
            text: text.to_owned(),
            urgent: i64::from(urgent),
            queued_at: Some(now.to_string()),
            delivered_turn: None,
        },
    )
    .map_err(store)?;
    if urgent {
        let pressed = press(conn, deliverer, session)?;
        if pressed != Delivery::Queued {
            return Ok(pressed);
        }
    }
    let notice = match &slot.ticket {
        Some(ticket) => format!("{ticket} has a message for you. End your turn to receive it."),
        None => "You have a message. End your turn to receive it.".to_owned(),
    };
    if deliverer.interrupt(session, Reply::text(Exit::Success, notice)) {
        Ok(Delivery::WaitEnded)
    } else {
        Ok(Delivery::Queued)
    }
}

/// Ends `session`'s running turn when an urgent message waits for it, so the next turn starts
/// with the message at once. The CLI loop calls it on every pass, which finishes the job for a
/// message that waited on a push, or arrived while the turn's supervisor was still starting.
///
/// Answers [`Delivery::Queued`] when nothing needs ending: no urgent message waits, or no turn
/// runs (the next turn carries the message anyway), or the turn already ended or was already
/// ended for a message.
pub fn press(
    conn: &Connection,
    deliverer: &dyn Deliverer,
    session: &SessionId,
) -> Result<Delivery> {
    let urgent = cli_slots::undelivered(conn, session.as_str())
        .map_err(store)?
        .iter()
        .any(|m| m.urgent != 0);
    if !urgent {
        return Ok(Delivery::Queued);
    }
    let Some(run) = running_turn(conn, session)?.and_then(|turn| turn.run) else {
        return Ok(Delivery::Queued);
    };
    let dir = Path::new(&run);
    if dir.join("result.json").exists() || was_cut_off(dir) {
        return Ok(Delivery::Queued);
    }
    if deliverer.land_pushing(session) {
        return Ok(Delivery::HeldForPush);
    }
    fs::write(dir.join(CUT_OFF_MARK), b"").map_err(|e| Error::Io(e.to_string()))?;
    deliverer
        .end_run(dir)
        .map_err(|e| Error::Io(format!("could not end {run}: {e}")))?;
    Ok(Delivery::TurnEnded(run))
}

/// The session's latest turn, while its slot says that turn is starting or running.
fn running_turn(conn: &Connection, session: &SessionId) -> Result<Option<CliTurn>> {
    let Some(slot) = cli_slots::by_session(conn, session.as_str()).map_err(store)? else {
        return Ok(None);
    };
    if !matches!(slot.state.as_str(), "starting" | "running") {
        return Ok(None);
    }
    let turns = cli_slots::turns(conn, session.as_str()).map_err(store)?;
    Ok(turns.into_iter().rfind(|t| t.turn == slot.turn))
}

#[allow(clippy::needless_pass_by_value)] // Called as `map_err(store)`.
fn store(error: ignite_store::StoreError) -> Error {
    Error::Io(error.to_string())
}

// Named for its file, so `cargo nextest run clid_messages` selects these tests.
#[cfg(test)]
#[path = "clid_messages_tests.rs"]
mod clid_messages_tests;
