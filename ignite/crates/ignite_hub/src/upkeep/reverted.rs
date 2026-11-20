//! Landings a tip round reverted because the tip did not build with them (`POST /reverted`).
//!
//! - The ticket that landed the commits is reopened at priority 0, in one batch with its
//!   metadata [`KEY`] (the revert commit, the landed commits and the failing checks) and a
//!   comment that gives the check, its output and the command that applies the work again. The
//!   daemon's dispatcher offers it to the worker whose last ticket it was.
//! - Commits that no landing recorded get a new priority 0 ticket with the same text.
//! - A second report of the same revert commit changes nothing.
//! - Each session holding a ticket that depends on the reopened one is told, with the
//!   mechanism `ignite tell` uses, to release it: the reopened ticket is live again, so the
//!   dependent ticket waits for it, and its code is gone from the tip.
//! - A landing of the ticket clears the metadata (`routes::dispatch::landed`).

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use ignite_core::protocol::hub::Reverted;
use ignite_core::protocol::tickets::{
    BatchRequest, EdgeKind, Operation, Status, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{edges, tickets};
use ignite_store::{Result, StoreError};

use super::filing::{self, Author};
use super::regressions::OUTPUT_LINES;

/// The ticket metadata key that holds what a tip round reverted.
pub const KEY: &str = "reverted";

/// The ticket metadata key that holds the number of the comment `ignite tell` delivers.
const TELL_KEY: &str = "tell";

/// Reopens the ticket of a reverted landing, or files a new ticket for commits no landing
/// recorded. Returns the ticket.
pub fn file(
    conn: &mut Connection,
    author: Author,
    report: &Reverted,
    now: Timestamp,
) -> Result<String> {
    let revert = report.revert.to_string();
    let text = details(report);
    let mut ops = Vec::new();
    let existing = match &report.ticket {
        None => None,
        Some(id) => {
            let ticket = tickets::load(conn, id.as_str())?
                .ok_or_else(|| StoreError::Rejected(format!("no ticket {id}")))?;
            let repeated = ticket
                .metadata
                .get(KEY)
                .and_then(|v| v.get("revert"))
                .and_then(Value::as_str)
                == Some(revert.as_str());
            if repeated {
                return Ok(ticket.id.to_string());
            }
            Some(ticket.id.to_string())
        }
    };
    let meta = json!({
        "revert": revert,
        "landed": report.landed.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "checks": report.checks.iter().map(ToString::to_string).collect::<Vec<_>>(),
    });
    match &existing {
        Some(id) => {
            ops.push(Operation::Reopen { id: id.clone() });
            ops.push(Operation::Update {
                id: id.clone(),
                set: Some(TicketFields {
                    priority: Some(0),
                    assignee: Some(None),
                    ..TicketFields::default()
                }),
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: BTreeMap::from([(KEY.to_owned(), meta)]),
                unset_meta: Vec::new(),
            });
            ops.push(Operation::Comment {
                id: id.clone(),
                text: text.clone(),
            });
            ops.extend(tell_dependents(conn, id, report)?);
        }
        None => ops.push(Operation::Create {
            fields: TicketFields {
                title: Some(format!(
                    "Reverted: {}",
                    report
                        .checks
                        .first()
                        .map(ToString::to_string)
                        .unwrap_or_default()
                )),
                // Released at once, like a regression: the tip is already reverted, and the
                // work waits for whoever picks this up.
                status: Some(Status::Open),
                body: Some(text),
                ticket_type: Some(TicketType::Bug),
                priority: Some(0),
                labels: Some(vec!["reverted".to_owned()]),
                owner: Some(filing::origin(conn, author.machine)?),
                metadata: Some(BTreeMap::from([(KEY.to_owned(), meta)])),
                ..TicketFields::default()
            },
            parent: None,
            deps: Vec::new(),
            request_key: Some(format!("reverted:{revert}")),
        }),
    }
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply_automatic(conn, &request, author.actor, author.machine, now)? {
        BatchOutcome::Applied(reply) => {
            Ok(existing.unwrap_or_else(|| reply.results[0].id.to_string()))
        }
        BatchOutcome::Conflict(conflict) => Err(StoreError::Rejected(format!(
            "filing the revert of {revert} conflicted at operation {}",
            conflict.failed
        ))),
    }
}

/// The ticket text: what happened, where, the end of the output and how to get the work back.
fn details(report: &Reverted) -> String {
    let checks: Vec<String> = report.checks.iter().map(ToString::to_string).collect();
    let landed: Vec<String> = report.landed.iter().map(ToString::to_string).collect();
    let lines: Vec<&str> = report.output.lines().collect();
    let tail = lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n");
    format!(
        "A tip round reverted this landing: the tip did not build with it ({checks}).\n\
         culprit: {search}\n\
         landed {landed}\n\
         reverted by {revert}, found on {machine}\n\
         rerun: ignite test {first}\n\n\
         output (last {OUTPUT_LINES} lines):\n{tail}\n\n\
         apply your work again with: git revert --no-edit {revert}",
        checks = checks.join(", "),
        search = report.search.words(),
        landed = landed.join(", "),
        revert = report.revert,
        machine = report.machine,
        first = checks.first().map_or("", String::as_str),
    )
}

/// The operations that tell the session holding each ticket that depends on `reverted` to
/// release it: a comment, and the `tell` metadata key set to its number, which the dispatcher
/// of the holding machine delivers (see `ignite tell`).
fn tell_dependents(conn: &Connection, reverted: &str, report: &Reverted) -> Result<Vec<Operation>> {
    let mut select = conn.prepare("SELECT src, kind FROM edges WHERE dst = ?1 ORDER BY src")?;
    let sources = select
        .query_map(params![reverted], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut ops = Vec::new();
    for (src, kind) in sources {
        if [
            edges::kind_name(EdgeKind::Related),
            edges::kind_name(EdgeKind::DiscoveredFrom),
        ]
        .contains(&kind.as_str())
        {
            continue;
        }
        let Some(dependent) = tickets::load(conn, &src)? else {
            continue;
        };
        if dependent.status != Status::InProgress || dependent.assignee.is_none() {
            continue;
        }
        let n: i64 = conn.query_row(
            "SELECT COALESCE(MAX(n), 0) + 1 FROM comments WHERE ticket = ?1",
            params![src],
            |row| row.get(0),
        )?;
        ops.push(Operation::Comment {
            id: src.clone(),
            text: format!(
                "{reverted}, which this ticket depends on, was reverted because the tip did not \
                 build with it ({checks}). Its code is gone from the tip. Release this ticket \
                 with `ignite release`; it is handed out again when {reverted} lands.",
                checks = report
                    .checks
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        });
        ops.push(Operation::Update {
            id: src,
            set: None,
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: BTreeMap::from([(TELL_KEY.to_owned(), json!(n))]),
            unset_meta: Vec::new(),
        });
    }
    Ok(ops)
}
