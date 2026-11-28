//! Tickets held in progress too long, reported for the justiciar to look at.
//!
//! A ticket is reported when it has been in progress for more than six hours while its session
//! is not active, for more than a day even while it is, or for more than 30 minutes by a session
//! that has stopped (its title starts with ⛔) or been idle that long.
//!
//! A session is active when its worker is `working` and has a command running, or recorded one
//! within the last 30 minutes. On a slow machine a card's landing gate alone can run for hours,
//! and a justiciar that read a six-hour report on such a ticket released it while its landing
//! gate was passing. The next worker then started again hours later, from nothing. An active
//! session is left alone until the day-long limit, which still catches one that loops.

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use ignite_core::protocol::tickets::{Graph, Status, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, session_titles, tickets};

use crate::upkeep::filing::{self, Filing, UPKEEP};
use crate::upkeep::parse_time;
use crate::upkeep::signature::Signature;

/// How long a ticket may stay in progress before a ticket reports it, in seconds.
pub const HELD_SECONDS: f64 = 6.0 * 3_600.0;

/// How long a ticket may stay in progress with a stopped or idle session, in seconds.
pub const STOPPED_SECONDS: f64 = 30.0 * 60.0;

/// How long upkeep waits before re-filing a `held` ticket for the same subject after a session
/// closes one without releasing the held ticket, in seconds.
pub const HELD_REFILE_COOLDOWN_SECONDS: f64 = 2.0 * 3_600.0;

/// The mark a session puts at the start of its title when it stops unfinished.
const STOPPED_MARK: &str = "⛔";

/// How long a ticket may stay in progress while its session is active, in seconds.
pub const ACTIVE_HELD_SECONDS: f64 = 24.0 * 3_600.0;

/// The label on a ticket for the justiciar to look at.
const JUSTICIAR: &str = ignite_core::dispatch::JUSTICIAR;


/// Why a ticket in progress is reported.
enum HeldFor {
    /// It has been in progress for more than [`HELD_SECONDS`] with a session that is not
    /// active, or for more than [`ACTIVE_HELD_SECONDS`].
    Long,
    /// Its session's title starts with the stopped mark.
    Stopped,
    /// Its session has been idle for more than [`STOPPED_SECONDS`].
    Idle,
}

/// A ticket in progress too long: see [`HeldFor`].
pub(super) struct Held {
    pub(super) ticket: String,
    title: String,
    minutes: u64,
    session: String,
    machine: String,
    reason: HeldFor,
    /// Minutes since the session last recorded a command, if it has recorded one.
    last_command: Option<u64>,
}

pub(super) fn find_held(conn: &Connection, graph: &Graph, now: Timestamp) -> Result<Vec<Held>> {
    let mut found = Vec::new();
    for entry in &graph.tickets {
        let ticket = &entry.ticket;
        let Some(claimed) = ticket
            .claimed_at
            .filter(|_| ticket.status == Status::InProgress)
        else {
            continue;
        };
        let session = ticket.session.as_ref().map(ToString::to_string);
        let last_command = match &session {
            Some(session) => minutes_since_command(conn, session, claimed, now)?,
            None => None,
        };
        let held = now.seconds_since(claimed);
        let reason = if held > ACTIVE_HELD_SECONDS {
            Some(HeldFor::Long)
        } else if held > HELD_SECONDS {
            let active = match &session {
                Some(session) => active(conn, session, last_command)?,
                None => false,
            };
            (!active).then_some(HeldFor::Long)
        } else if held > STOPPED_SECONDS {
            stopped_or_idle(conn, session.clone(), now)?
        } else {
            None
        };
        let Some(reason) = reason else { continue };
        found.push(Held {
            ticket: ticket.id.to_string(),
            title: ticket.title.clone(),
            minutes: (held / 60.0) as u64,
            session: session
                .or_else(|| ticket.assignee.clone())
                .unwrap_or_else(|| "unknown".to_owned()),
            machine: ticket
                .machine
                .as_ref()
                .map_or_else(|| "unknown".to_owned(), ToString::to_string),
            reason,
            last_command,
        });
    }
    Ok(found)
}

/// Whether `session` is working on its ticket now: its worker is `working` and either has a
/// command running or recorded one within [`STOPPED_SECONDS`] (`last_command`, in minutes).
fn active(conn: &Connection, session: &str, last_command: Option<u64>) -> Result<bool> {
    let running: Option<bool> = conn
        .query_row(
            "SELECT command_running FROM workers WHERE session = ?1 AND state = 'working'
             ORDER BY command_running DESC LIMIT 1",
            [session],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match running {
        None => false,
        Some(running) => {
            running || last_command.is_some_and(|minutes| minutes as f64 * 60.0 <= STOPPED_SECONDS)
        }
    })
}

/// Minutes since `session` last recorded a command, counting only commands since `claimed`.
/// The records cover every `ignite` command a session runs, including `ignite check`, whose
/// records name no ticket.
fn minutes_since_command(
    conn: &Connection,
    session: &str,
    claimed: Timestamp,
    now: Timestamp,
) -> Result<Option<u64>> {
    let at: Option<String> = conn.query_row(
        "SELECT max(at) FROM records WHERE at >= ?2 AND session = ?1",
        [session, &claimed.to_string()],
        |row| row.get(0),
    )?;
    let Some(at) = at else { return Ok(None) };
    Ok(Some((now.seconds_since(parse_time(&at)?).max(0.0) / 60.0) as u64))
}

/// Whether a session (not upkeep) closed the latest `held` ticket for `subject` within
/// [`HELD_REFILE_COOLDOWN_SECONDS`] of `now`, so upkeep waits before re-filing it.
pub(super) fn held_recently_closed(conn: &Connection, subject: &str, now: Timestamp) -> Result<bool> {
    let key = Signature::of_subject(subject, "held").key();
    let newest: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT id, closed_at, metadata FROM tickets
             WHERE json_extract(metadata, '$.ignite_signature') = ?1
               AND status = 'closed' AND closed_at IS NOT NULL
             ORDER BY closed_at DESC, id DESC LIMIT 1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((id, closed_at, metadata)) = newest else {
        return Ok(false);
    };
    if now.seconds_since(parse_time(&closed_at)?) >= HELD_REFILE_COOLDOWN_SECONDS {
        return Ok(false);
    }
    let auto_closed = metadata
        .as_deref()
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
        .is_some_and(|meta| meta.get(filing::AUTO_CLOSED_KEY).is_some());
    if auto_closed {
        return Ok(false);
    }
    let closed_by_upkeep = tickets::history(conn, &id)?
        .into_iter()
        .rev()
        .find(|row| row.change.pointer("/status/1").and_then(Value::as_str) == Some("closed"))
        .is_some_and(|row| row.actor == UPKEEP.actor);
    Ok(!closed_by_upkeep)
}

/// Whether the session holding a ticket has had a title starting with the stopped mark, or been
/// idle, for more than [`STOPPED_SECONDS`].
fn stopped_or_idle(
    conn: &Connection,
    session: Option<String>,
    now: Timestamp,
) -> Result<Option<HeldFor>> {
    let Some(session) = session else {
        return Ok(None);
    };
    let longer = |since: &str| -> Result<bool> {
        Ok(now.seconds_since(parse_time(since)?) > STOPPED_SECONDS)
    };
    if let Some((title, updated)) = session_titles::get(conn, &session)?
        && title.starts_with(STOPPED_MARK)
        && longer(&updated)?
    {
        return Ok(Some(HeldFor::Stopped));
    }
    if let Some(since) = fleet::idle_since(conn, &session)?
        && longer(&since)?
    {
        return Ok(Some(HeldFor::Idle));
    }
    Ok(None)
}

pub(super) fn held_filing(held: &Held) -> Filing {
    let Held {
        ticket,
        title,
        minutes,
        session,
        machine,
        reason,
        last_command,
    } = held;
    let hours = minutes / 60;
    let (heading, how) = match reason {
        HeldFor::Long => (
            format!("Ticket {ticket} has been in progress for {hours} hours"),
            format!("held for {hours} hours by session {session} on machine {machine}"),
        ),
        HeldFor::Stopped => (
            format!("Ticket {ticket} is held by a stopped session"),
            format!(
                "held {minutes} min by a session titled {STOPPED_MARK}: {session} on machine \
                 {machine}"
            ),
        ),
        HeldFor::Idle => (
            format!("Ticket {ticket} is held by an idle session"),
            format!("held {minutes} min by an idle session: {session} on machine {machine}"),
        ),
    };
    let last = match last_command {
        Some(minutes) => format!(" Its session last recorded a command {minutes} min ago."),
        None => " Its session has recorded no command.".to_owned(),
    };
    Filing {
        signature: Signature::of_subject(ticket, "held"),
        evidence: "held".to_owned(),
        fields: TicketFields {
            title: Some(heading),
            body: Some(format!(
                "{ticket} ({title}) is {how}.{last} A session that stopped, a worker that lost the \
                 ticket or a ticket too large for one session leaves it so. Find out which, and \
                 release, split or finish the ticket."
            )),
            ticket_type: Some(TicketType::Task),
            priority: Some(1),
            labels: Some(vec![JUSTICIAR.to_owned()]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    }
}
