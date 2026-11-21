//! Faults of one machine, and tickets that no machine can take.
//!
//! - A failure that one machine stored and another machine then passed on the same inputs
//!   belongs to the first machine's own state, such as a stale cache or a broken tool. The pass
//!   replaces the failure in the result cache, and the first machine gets a P1 ticket pinned to it
//!   ([`disagreement`]).
//! - [`unassignable`] reports live tickets pinned to an unregistered machine or unknown provider.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, pins};

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
            labels: Some(vec!["machine".to_owned(), machine_label(&failed)]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    filing::file(conn, author, &filing, now)
}

/// Every live ticket with an unknown machine or provider pin, ordered by ticket.
pub fn unassignable(conn: &Connection) -> Result<Vec<(String, String)>> {
    let machines = fleet::machines(conn)?;
    let settings = fleet_settings::settings(conn)?;
    let providers: BTreeSet<String> = settings
        .providers
        .into_keys()
        .chain(machines.iter().flat_map(|machine| {
            serde_json::from_str::<Vec<String>>(&machine.cli_apps).unwrap_or_default()
        }))
        .map(|provider| provider.to_ascii_lowercase())
        .collect();
    let mut select = conn.prepare(
        r#"WITH RECURSIVE pinned(id) AS (
            SELECT id FROM tickets WHERE labels LIKE '%"machine:%' OR labels LIKE '%"provider:%'
            UNION
            SELECT t.id FROM tickets t JOIN pinned p ON t.parent = p.id
         )
         SELECT p.id FROM pinned p JOIN tickets t ON t.id = p.id
         WHERE t.status != 'closed' ORDER BY p.id"#,
    )?;
    let ids = select.query_map([], |row| row.get::<_, String>(0))?;
    let mut stranded = Vec::new();
    for id in ids {
        let id = id?;
        let pins = pins::of(conn, &id)?;
        let mut reasons = Vec::new();
        if let Some(machine) = pins.machine
            && !machines
                .iter()
                .any(|entry| entry.name.eq_ignore_ascii_case(&machine))
        {
            reasons.push(format!(
                "pinned to {machine}, which is not a registered machine"
            ));
        }
        if let Some(provider) = pins.provider
            && !providers.contains(&provider.to_ascii_lowercase())
        {
            reasons.push(format!(
                "pinned to {provider}, which is not a known provider"
            ));
        }
        if !reasons.is_empty() {
            stranded.push((id, reasons.join("; ")));
        }
    }
    Ok(stranded)
}
