//! Faults of one machine, and tickets that no machine can take.
//!
//! - A failure that one machine stored and another machine then passed on the same inputs
//!   belongs to the first machine's own state, such as a stale cache or a broken tool. The pass
//!   replaces the failure in the result cache, and the first machine gets a P1 ticket labelled
//!   `needs:<its name>`, so only it can work the ticket ([`disagreement`]).
//! - A live ticket with a `needs:` label that names neither the operating system nor the name of
//!   any registered machine can never be dispatched. [`unassignable`] lists those tickets for the
//!   first line of the dashboard. The hub holds no check manifests, so the families a ticket's
//!   files bring in are not considered.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::protocol::hub::ResultKey;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::fleet;

use super::filing::{self, Author, Filed, Filing};
use super::signature::Signature;

/// A machine's registered name, or its ID when it never registered.
pub fn name(conn: &Connection, machine: &str) -> Result<String> {
    let name: Option<String> = conn
        .query_row(
            "SELECT name FROM machines WHERE id = ?1",
            params![machine],
            |row| row.get(0),
        )
        .optional()?;
    Ok(name.unwrap_or_else(|| machine.to_owned()))
}

/// A cached failure that another machine then passed.
#[derive(Clone, Debug)]
pub struct Disagreement<'a> {
    pub key: &'a ResultKey,
    /// The machine whose failure was replaced.
    pub failed_on: &'a str,
    /// Where that machine saved the failure's output, if it said.
    pub failed_output: Option<&'a str>,
    pub passed_on: &'a str,
}

/// Files the P1 ticket for the machine whose failure another machine passed. Repeats on the
/// same machine comment on its open ticket.
pub fn disagreement(
    conn: &mut Connection,
    author: Author,
    seen: &Disagreement,
    now: Timestamp,
) -> Result<Filed> {
    let failed = name(conn, seen.failed_on)?;
    let passed = name(conn, seen.passed_on)?;
    let check = seen.key.check.as_str();
    let output = seen.failed_output.map_or(String::new(), |path| {
        format!(" Its output was saved at {path} on {failed}.")
    });
    let body = format!(
        "{check} failed on {failed} ({}) and then passed on {passed} ({}) on the same inputs \
         (input hash {}). The failure belongs to {failed}'s own state, such as a stale cache or \
         a broken tool, so the pass replaced it in the result cache.{output} Find what on \
         {failed} makes the check fail and fix it there.",
        seen.failed_on, seen.passed_on, seen.key.hash
    );
    let filing = Filing {
        signature: Signature::of_subject(seen.failed_on, "disagreement"),
        evidence: format!("{check}@{}", seen.key.hash),
        fields: TicketFields {
            title: Some(format!("Checks fail only on {failed}")),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(1),
            labels: Some(vec!["machine".to_owned(), format!("needs:{failed}")]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    filing::file(conn, author, &filing, now)
}

/// Every live ticket that no registered machine can take, with the reason, ordered by ticket.
/// A `needs:` label must name the operating system or the name of some machine; `needs:person`
/// asks for a person, not a machine, and is left out.
pub fn unassignable(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut known = BTreeSet::new();
    for machine in fleet::machines(conn)? {
        known.insert(machine.os);
        known.insert(machine.name);
    }
    let mut select = conn.prepare(
        "SELECT t.id, l.value FROM tickets t, json_each(t.labels) l
         WHERE t.status != 'closed' AND l.value LIKE 'needs:%' AND l.value != 'needs:person'
         ORDER BY t.id, l.value",
    )?;
    let needs = select
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(needs
        .into_iter()
        .filter(|(_, label)| {
            label
                .strip_prefix("needs:")
                .is_some_and(|need| !known.contains(need))
        })
        .map(|(ticket, label)| {
            let reason = format!("it needs {label}, which no registered machine has");
            (ticket, reason)
        })
        .collect())
}
