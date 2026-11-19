//! Faults of one machine, and tickets that no machine can take.
//!
//! - A failure that one machine stored and another machine then passed on the same inputs
//!   belongs to the first machine's own state, such as a stale cache or a broken tool. The pass
//!   replaces the failure in the result cache, and the first machine gets a P1 ticket that it
//!   owns, so only its workers can work the ticket ([`disagreement`]).
//! - A live ticket whose owner names a machine or app that `ignite.toml` does not declare can
//!   never be dispatched. [`unassignable`] lists those tickets for the first line of the
//!   dashboard. The hub holds no check manifests, so the families a ticket's files bring in are
//!   not considered.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::owner::Owner;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::owners;

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
            labels: Some(vec!["machine".to_owned()]),
            owner: Some(Some(Owner::new(&failed, None))),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    filing::file(conn, author, &filing, now)
}

/// Every live ticket that no worker can take, with the reason, ordered by ticket: its own owner
/// names a machine that `ignite.toml` does not declare, or an app the pool does not run. A hub
/// that has recorded no routing facts reports nothing.
pub fn unassignable(conn: &Connection) -> Result<Vec<(String, String)>> {
    let routing = owners::routing(conn)?;
    if routing.machines.is_empty() {
        return Ok(Vec::new());
    }
    let mut select = conn.prepare(
        "SELECT id, owner FROM tickets WHERE status != 'closed' AND owner IS NOT NULL ORDER BY id",
    )?;
    let owned = select
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(owned
        .into_iter()
        .filter_map(|(ticket, owner)| {
            let reason = match owner.parse::<Owner>() {
                Err(_) => format!("its owner {owner:?} is not a machine and app"),
                Ok(parsed) if routing.machine(&parsed.machine).is_none() => {
                    format!("it is owned by {owner}, a machine ignite.toml does not declare")
                }
                Ok(Owner { app: Some(app), .. }) if !routing.apps.contains(&app) => {
                    format!("it is owned by {owner}, and the pool runs no {app} workers")
                }
                Ok(_) => return None,
            };
            Some((ticket, reason))
        })
        .collect())
}
