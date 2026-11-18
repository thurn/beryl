//! The `machines` page: each machine's cores, what its recent commands spent by class, its
//! trees with the tickets in them, when it was last seen and which Ignite it runs.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

use super::attention::{MACHINE_SILENT_SECONDS, age, frame, seconds_since};
use super::components::{self as c, PageOptions, Tone, esc};
use super::data::ReadError;
use super::references::{display_id, ticket_url};

/// A row of `machines`: ID, name, system, cores, memory, version and when it was last seen.
type MachineRow = (String, String, String, i64, i64, String, String);

/// Commands newer than this count toward a machine's work by class.
const RECENT_SECONDS: f64 = 86_400.0;

/// One machine's section: its state, its work in the last day by class, and its trees.
fn machine_section(
    conn: &Connection,
    root: &str,
    now: Timestamp,
    machine: &MachineRow,
) -> Result<String, ReadError> {
    let (id, name, os, cores, memory_mb, version, last_seen) = machine;
    let since = now.plus_seconds(-RECENT_SECONDS).to_string();
    let silent = seconds_since(now, Some(last_seen)).is_none_or(|s| s > MACHINE_SILENT_SECONDS);
    let state = if silent {
        c::dot(Tone::Red, "Not reporting", None)
    } else {
        c::dot(Tone::Green, "Reporting", None)
    };
    let mut by_class = conn.prepare(
        "SELECT class, COUNT(*), SUM(cpu_s) FROM records
             WHERE machine = ?1 AND at >= ?2 GROUP BY class ORDER BY class",
    )?;
    let classes = by_class
        .query_map([id, &since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let spent = if classes.is_empty() {
        c::muted("No commands in the last day")
    } else {
        classes
            .iter()
            .map(|(class, count, cpu)| {
                format!("{} {count} ({:.0} core-minutes)", esc(class), cpu / 60.0)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut trees = conn.prepare(
        "SELECT tree, id, status, title FROM tickets
             WHERE machine = ?1 AND tree IS NOT NULL AND status != 'closed' ORDER BY tree",
    )?;
    let trees = trees
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let tree_rows = if trees.is_empty() {
        c::empty("No tree holds a ticket.")
    } else {
        c::rows(trees.iter().map(|(tree, ticket, status, title)| {
            let label = c::link(
                &ticket_url(root, ticket),
                &esc(&display_id(ticket)),
                None,
                "",
            );
            c::row(
                &(c::mono(tree)
                    + &c::muted(" · ")
                    + &label
                    + &c::muted(&format!(" {}", esc(title)))),
                &esc(status),
                None,
            )
        }))
    };
    let items = vec![
        ("State".to_owned(), state),
        ("System".to_owned(), esc(os)),
        (
            "Cores".to_owned(),
            format!("{cores}, {} GB memory", memory_mb / 1024),
        ),
        (
            "Last seen".to_owned(),
            esc(&age(now, Some(last_seen))) + " ago",
        ),
        ("Version".to_owned(), c::mono(version)),
        ("Last day, by class".to_owned(), spent),
    ];
    Ok(c::section(
        name,
        &(c::meta(&items) + &c::subsection("Trees", &tree_rows, Some(trees.len()), 4)),
        None,
        None,
        3,
    ))
}

/// The page for the router.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let mut machines = conn.prepare(
        "SELECT id, name, os, cores, memory_mb, version, last_seen FROM machines ORDER BY name, id",
    )?;
    let machines = machines
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut body = String::new();
    for machine in &machines {
        body += &machine_section(conn, root, now, machine)?;
    }
    if machines.is_empty() {
        body = c::empty("No machine has registered.");
    }
    Ok(frame(
        root,
        seq,
        "machines",
        "Cores, trees and what each machine runs.",
        &body,
    ))
}
