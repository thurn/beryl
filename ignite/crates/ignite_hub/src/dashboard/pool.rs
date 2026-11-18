//! The `pool` page: each machine's workers with their tickets and the command they run, the
//! dispatcher's state per app, and the `epics` page with each epic's share of the workers.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;
use ignite_store::hub::tickets;

use super::attention::{STUCK_SECONDS, age, frame, seconds_since};
use super::components::{self as c, PageOptions, Tone, esc};
use super::data::ReadError;
use super::references::{display_id, ticket_url};

/// A worker as the page shows it.
struct Worker {
    session: String,
    app: String,
    role: String,
    state: String,
    ticket: Option<String>,
    command: Option<String>,
    command_since: Option<String>,
    command_running: bool,
    claimed_at: Option<String>,
}

/// What is wrong with a worker's command, if anything: a command that has run for over ten
/// minutes, or a worker holding a ticket that has run no command for that long.
pub fn command_flag(
    worker_running: bool,
    holds_ticket: bool,
    since_seconds: Option<f64>,
) -> Option<&'static str> {
    let long = since_seconds.is_some_and(|seconds| seconds > STUCK_SECONDS);
    match (worker_running, holds_ticket, long) {
        (true, _, true) => Some("Running over 10 minutes"),
        (false, true, true) => Some("No command for over 10 minutes"),
        _ => None,
    }
}

fn dispatcher_tone(state: &str) -> Tone {
    match state {
        "down" => Tone::Red,
        "paused" => Tone::Amber,
        _ => Tone::Green,
    }
}

/// A session ID cut to a length that fits a phone's width; the full ID is its tooltip.
fn shortened(session: &str) -> String {
    const SHOWN: usize = 14;
    if session.chars().count() <= SHOWN {
        return session.to_owned();
    }
    session.chars().take(SHOWN).collect::<String>() + "…"
}

fn worker_row(root: &str, now: Timestamp, worker: &Worker) -> String {
    let since = worker
        .command_since
        .as_deref()
        .or(worker.claimed_at.as_deref());
    let flag = command_flag(
        worker.command_running,
        worker.ticket.is_some(),
        seconds_since(now, since),
    );
    let ticket = worker.ticket.as_ref().map_or_else(
        || c::muted("no ticket"),
        |id| c::link(&ticket_url(root, id), &esc(&display_id(id)), None, ""),
    );
    let command = match (&worker.command, &worker.command_since) {
        (Some(command), Some(started)) => {
            let verb = if worker.command_running {
                "running for"
            } else {
                "ended"
            };
            let how_long = age(now, Some(started));
            let ago = if worker.command_running {
                how_long
            } else {
                format!("{how_long} ago")
            };
            c::mono(&format!("ignite {command}")) + &c::muted(&format!(" {verb} {}", esc(&ago)))
        }
        _ => c::muted("no command yet"),
    };
    let mark = flag.map_or(String::new(), |text| c::chip(text, Tone::Amber, None));
    let tone = if worker.state == "down" || worker.state == "stalled" {
        Tone::Red
    } else if worker.state == "idle" {
        Tone::Muted
    } else {
        Tone::Green
    };
    c::row(
        &(c::dot(tone, &worker.state, None)
            + " "
            + &c::tip(&c::mono(&shortened(&worker.session)), &worker.session)
            + &c::muted(" · ")
            + &ticket
            + "<div>"
            + &command
            + "</div>"),
        &format!("{} {}", esc(&worker.app), mark),
        None,
    )
}

/// The `pool` page.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let mut machines = conn.prepare("SELECT id, name FROM machines ORDER BY name, id")?;
    let machines = machines
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut statement = conn.prepare(
        "SELECT w.session, w.app, w.role, w.state, w.ticket, w.command, w.command_since,
                w.command_running, t.claimed_at
         FROM workers w LEFT JOIN tickets t ON t.id = w.ticket
         WHERE w.machine = ?1 ORDER BY w.session",
    )?;
    let mut body = String::new();
    for (id, name) in &machines {
        let workers = statement
            .query_map([id], |row| {
                Ok(Worker {
                    session: row.get(0)?,
                    app: row.get(1)?,
                    role: row.get(2)?,
                    state: row.get(3)?,
                    ticket: row.get(4)?,
                    command: row.get(5)?,
                    command_since: row.get(6)?,
                    command_running: row.get(7)?,
                    claimed_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let dispatchers: Vec<String> = workers
            .iter()
            .filter(|worker| worker.role == "dispatcher")
            .map(|worker| {
                c::dot(
                    dispatcher_tone(&worker.state),
                    &format!("{} dispatcher {}", worker.app, worker.state),
                    None,
                )
            })
            .collect();
        let dispatchers = if dispatchers.is_empty() {
            c::dot(Tone::Red, "No dispatcher", None)
        } else {
            dispatchers.join(" ")
        };
        let members: Vec<&Worker> = workers
            .iter()
            .filter(|worker| worker.role == "worker")
            .collect();
        let list = if members.is_empty() {
            c::empty("No workers.")
        } else {
            c::rows(members.iter().map(|worker| worker_row(root, now, worker)))
        };
        body += &c::section(
            name,
            &(format!("<p>{dispatchers}</p>") + &list),
            Some(members.len()),
            None,
            3,
        );
    }
    if machines.is_empty() {
        body = c::empty("No machine has registered.");
    }
    Ok(frame(
        root,
        seq,
        "pool",
        "Workers, what they hold and what they are running.",
        &body,
    ))
}

/// Descendants of an epic have IDs that continue its ID after a dot.
fn counts(
    conn: &Connection,
    epic: &str,
    ready: &[String],
) -> Result<(i64, usize, i64, i64), ReadError> {
    let like = format!("{}.%", epic.replace('%', "\\%").replace('_', "\\_"));
    let in_progress: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tickets WHERE id LIKE ?1 ESCAPE '\\' AND status = 'in_progress'
           AND NOT EXISTS (SELECT 1 FROM tickets c WHERE c.parent = tickets.id)",
        [&like],
        |row| row.get(0),
    )?;
    let prefix = format!("{epic}.");
    let ready_here = ready.iter().filter(|id| id.starts_with(&prefix)).count();
    let landed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tickets WHERE id LIKE ?1 ESCAPE '\\' AND landed_at IS NOT NULL",
        [&like],
        |row| row.get(0),
    )?;
    let unverified: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tickets WHERE id LIKE ?1 ESCAPE '\\' AND landed_at IS NOT NULL
           AND landed_at > COALESCE((SELECT MAX(verified) FROM tips WHERE state = 'verified'), '')",
        [&like],
        |row| row.get(0),
    )?;
    Ok((in_progress, ready_here, landed, unverified))
}

/// The `epics` page: which epics are active and their worker shares, with the tickets in
/// progress and ready and whether landed work is verified.
pub(crate) fn epics(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let ready: Vec<String> = tickets::ready(conn, now)?
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut statement = conn.prepare(
        "SELECT id, title, workers, max_workers FROM tickets
         WHERE status != 'closed' AND (workers IS NOT NULL OR type = 'epic')
           AND EXISTS (SELECT 1 FROM tickets c WHERE c.parent = tickets.id)
         ORDER BY workers IS NULL, id",
    )?;
    let epics = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = Vec::new();
    for (id, title, workers, max_workers) in &epics {
        let (in_progress, ready_here, landed, unverified) = counts(conn, id, &ready)?;
        let share = match (workers, max_workers) {
            (Some(workers), Some(cap)) => c::chip(
                &format!("{workers} workers, at most {cap}"),
                Tone::Green,
                None,
            ),
            (Some(workers), None) => c::chip(&format!("{workers} workers"), Tone::Green, None),
            _ => c::chip(
                "Inactive",
                Tone::Muted,
                Some("The epic has no workers number, so none of its tickets are offered."),
            ),
        };
        let verified = if landed == 0 {
            c::muted("nothing landed")
        } else if unverified == 0 {
            c::dot(Tone::Green, "landed work verified", None)
        } else {
            c::dot(
                Tone::Amber,
                &format!("{unverified} of {landed} landed not yet verified"),
                None,
            )
        };
        let main = c::link(
            &ticket_url(root, id),
            &esc(&format!("{} {title}", display_id(id))),
            None,
            "",
        );
        rows.push(c::row(
            &(main
                + &c::muted(&format!(
                    " · {in_progress} in progress · {ready_here} ready · "
                ))
                + &verified),
            &share,
            None,
        ));
    }
    let body = if rows.is_empty() {
        c::empty("No epics.")
    } else {
        c::rows(rows)
    };
    Ok(frame(
        root,
        seq,
        "epics",
        "Each epic's share of the workers, and its work in progress.",
        &body,
    ))
}
