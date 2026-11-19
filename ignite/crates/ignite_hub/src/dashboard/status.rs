//! The project status panel on the home page: who is working on what, and who holds each
//! lease. It is a fragment the page loads after it shows, and the script refreshes it when the
//! feed reports a change.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;
use ignite_store::hub::leases;

use super::common::{person, worker};
use super::components::{self as c, esc};
use super::data::ReadError;
use super::references::{display_id, ticket_url};

/// Open work with a session or person on it, newest claim first: the session, the ticket's ID
/// and title, and the machine.
fn active_sessions(conn: &Connection, root: &str) -> Result<String, ReadError> {
    let mut statement = conn.prepare(
        "SELECT COALESCE(session, assignee), id, title, COALESCE(machine, '') FROM tickets
         WHERE status = 'in_progress' AND COALESCE(session, assignee) IS NOT NULL
         ORDER BY claimed_at DESC, id",
    )?;
    let found = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if found.is_empty() {
        return Ok(c::empty("No active sessions reported."));
    }
    Ok(c::rows(found.iter().map(
        |(session, id, title, machine)| {
            let shown = if title.is_empty() {
                display_id(id)
            } else {
                title.clone()
            };
            let label = c::link(&ticket_url(root, id), &esc(&shown), None, "");
            c::row(
                &(worker(Some(session), id) + &c::muted(" · ") + &label),
                &esc(machine),
                None,
            )
        },
    )))
}

/// One panel per lease: who holds it, until when, and who waits.
fn lease_panels(conn: &Connection, now: Timestamp) -> Result<String, ReadError> {
    let held = leases::list(conn, now)?;
    if held.is_empty() {
        return Ok(c::subsection(
            "Leases",
            &c::empty("No lease is held."),
            None,
            3,
        ));
    }
    Ok(held
        .iter()
        .map(|lease| {
            let live = lease.expires_at > now;
            let holder = if live {
                person(Some(&lease.holder)) + &c::muted(" · ") + &esc(&lease.machine)
            } else {
                c::muted("Available")
            };
            let queue: Vec<String> = lease
                .waiters
                .iter()
                .map(|waiter| person(Some(&waiter.holder)))
                .collect();
            let mut items = vec![("Holder".to_owned(), holder)];
            if live {
                items.push((
                    "Expires".to_owned(),
                    c::time(Some(&lease.expires_at.to_string()), ""),
                ));
            }
            items.push((
                "Waiting".to_owned(),
                if queue.is_empty() {
                    c::muted("Nobody")
                } else {
                    queue.join(", ")
                },
            ));
            c::subsection(&format!("{} lease", lease.name), &c::meta(&items), None, 3)
        })
        .collect())
}

/// The status panel's HTML for the project at `root`.
pub fn fragment(conn: &Connection, root: &str, now: Timestamp) -> Result<String, ReadError> {
    Ok(
        c::subsection("Sessions", &active_sessions(conn, root)?, None, 3)
            + &lease_panels(conn, now)?,
    )
}
