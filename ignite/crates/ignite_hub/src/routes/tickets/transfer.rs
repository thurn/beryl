//! `GET /export` and `POST /import`: every ticket with its edges, comments, history and aliases,
//! 200 at a time or about 4 MiB, whichever is less, as the ticket service moved them for backups and restores.
//!
//! Export writes JSON Lines ordered by ID, starting after the `after` parameter, and an empty
//! body at the end. Given one or more `id` parameters instead, it writes those tickets (each that
//! exists), which is how each machine's ticket mirror fetches the tickets the feed says changed.
//! Import applies one page in one transaction: a ticket whose stored copy was updated later than
//! the imported one is skipped unless the request is forced, as a rollback from a mirror is, and
//! an imported ticket replaces the stored one's columns, edges, comments and history and adds its
//! aliases.

use std::collections::{BTreeMap, HashSet};

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, TransactionBehavior, params, params_from_iter};
use serde_json::{Value, json};

use ignite_core::ids::Seq;
use ignite_core::protocol::tickets::{FeedChange, ImportReply, ImportRequest, WholeTicket};
use ignite_store::hub::{edges, feed, tickets};
use ignite_store::{Result, StoreError};

use super::{failure, store_failure};
use crate::routes::Response;

/// The most tickets one page carries, either way.
const PAGE: usize = 200;

/// The most bytes of tickets one export response holds, after which the page ends early. The
/// ticket mirror's HTTP client refuses a body over 10 MiB, and 200 tickets with long histories
/// can exceed that, so a page stops once it has reached this size. A page always holds at least
/// one ticket, and a client continues after the last ticket it received.
const PAGE_BYTES: usize = 4 * 1024 * 1024;

/// Columns that hold JSON text.
const JSON_COLUMNS: &[&str] = &["labels", "metadata", "landed", "saving"];

/// One page of whole tickets after `after`, or the tickets `ids` names, as JSON Lines.
pub(super) fn export(
    conn: &Connection,
    after: Option<&str>,
    ids: &[&str],
) -> std::result::Result<Response, Response> {
    if ids.len() > PAGE {
        return Err(failure(
            400,
            &format!("At most {PAGE} tickets can be exported at once"),
        ));
    }
    let body = if ids.is_empty() {
        let found = page_ids(conn, after.unwrap_or_default());
        found.and_then(|found| written(conn, &found, PAGE_BYTES))
    } else {
        written(conn, ids, PAGE_BYTES)
    }
    .map_err(|e| store_failure(&e))?;
    Ok(Response {
        status: 200,
        content_type: "application/x-ndjson",
        body,
        headers: Vec::new(),
    })
}

/// The IDs of the next page of tickets after `after`.
fn page_ids(conn: &Connection, after: &str) -> Result<Vec<String>> {
    let mut statement =
        conn.prepare("SELECT id FROM tickets WHERE id > ?1 ORDER BY id LIMIT ?2")?;
    let ids = statement
        .query_map(params![after, PAGE as i64], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// Each ticket of `ids` that exists, in the order given, one JSON line each. Writing stops after
/// the line that brings the body to `budget` bytes, so the page is cut short; the tickets left
/// out are those after the last one written.
fn written<S: AsRef<str>>(conn: &Connection, ids: &[S], budget: usize) -> Result<Vec<u8>> {
    let mut body = String::new();
    for id in ids {
        let Some(whole) = whole_ticket(conn, id.as_ref())? else {
            continue;
        };
        body.push_str(&json!(whole).to_string());
        body.push('\n');
        if body.len() >= budget {
            break;
        }
    }
    Ok(body.into_bytes())
}

fn whole_ticket(conn: &Connection, id: &str) -> Result<Option<WholeTicket>> {
    let Some(ticket) = tickets::load(conn, id)? else {
        return Ok(None);
    };
    Ok(Some(WholeTicket {
        ticket,
        edges: edges::from(conn, id)?,
        comments: tickets::comments(conn, id)?,
        history: tickets::history(conn, id)?,
        aliases: aliases(conn, id)?,
    }))
}

fn aliases(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT alias FROM aliases WHERE id = ?1 ORDER BY alias")?;
    let rows = statement.query_map([id], |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Imports one page. Every ticket that changed gets the same new feed position as its version.
pub(super) fn import(
    conn: &mut Connection,
    request: &ImportRequest,
) -> std::result::Result<Response, Response> {
    if request.tickets.len() > PAGE {
        return Err(failure(
            400,
            &format!("At most {PAGE} tickets can be imported at once"),
        ));
    }
    let reply = (|| {
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let reply = import_page(&transaction, &request.tickets, request.force)?;
        transaction.commit()?;
        Ok(reply)
    })()
    .map_err(|e| store_failure(&e))?;
    Ok(Response::json(200, &json!(reply)))
}

fn import_page(conn: &Connection, page: &[WholeTicket], force: bool) -> Result<ImportReply> {
    let mut seen = HashSet::new();
    for whole in page {
        let id = whole.ticket.id.as_str();
        if !seen.insert(id) {
            return Err(StoreError::Rejected(format!("Duplicate ticket {id}")));
        }
        if whole.edges.iter().any(|edge| edge.src.as_str() != id) {
            return Err(StoreError::Rejected(format!(
                "Every edge of {id} must start at it"
            )));
        }
        if whole.comments.iter().any(|c| c.ticket.as_str() != id)
            || whole.history.iter().any(|h| h.ticket.as_str() != id)
        {
            return Err(StoreError::Rejected(format!(
                "Every row of {id} must name it"
            )));
        }
    }
    // Restored history keeps its own positions; later writes must number above all of them.
    let floor = page
        .iter()
        .flat_map(|whole| &whole.history)
        .map(|history| history.seq)
        .max()
        .unwrap_or_default();
    let current = feed::current(conn)?;
    let seq = Seq(current.max(floor).0 + 1);
    let mut changed = Vec::new();
    for whole in page {
        if import_ticket(conn, whole, seq, force)? {
            changed.push(whole.ticket.id.clone());
        }
    }
    check_parents(conn, page)?;
    if changed.is_empty() {
        return Ok(ImportReply {
            seq: current,
            imported: 0,
        });
    }
    feed::set_current(conn, seq)?;
    for id in &changed {
        feed::record(
            conn,
            &FeedChange {
                seq,
                kind: "ticket".to_owned(),
                ticket: Some(id.clone()),
                name: None,
            },
        )?;
    }
    Ok(ImportReply {
        seq,
        imported: u32::try_from(changed.len()).unwrap_or(u32::MAX),
    })
}

/// Writes one ticket and its rows; whether anything changed. A stored ticket updated later than
/// `whole` is left alone unless `force` is set.
fn import_ticket(conn: &Connection, whole: &WholeTicket, seq: Seq, force: bool) -> Result<bool> {
    let id = whole.ticket.id.as_str();
    let collides: Option<String> = conn
        .query_row(
            "SELECT id FROM aliases WHERE alias = ?1 AND id != ?1",
            [id],
            |row| row.get(0),
        )
        .ok();
    if let Some(other) = collides {
        return Err(StoreError::Rejected(format!(
            "Ticket {id} collides with an alias of {other}"
        )));
    }
    let mut ticket = whole.ticket.clone();
    let stored = tickets::load(conn, id)?;
    if let Some(stored) = &stored {
        if stored.updated_at > ticket.updated_at && !force {
            return Ok(false);
        }
        ticket.next_child = ticket.next_child.max(stored.next_child);
        ticket.version = stored.version;
    }
    let mut changed = false;
    if stored.as_ref() != Some(&ticket) {
        ticket.version = seq;
        write_ticket(conn, &json!(ticket))?;
        changed = true;
    }
    let mut edges = whole.edges.clone();
    edges.sort_by(|a, b| a.dst.as_str().cmp(b.dst.as_str()));
    if edges::from(conn, id)? != edges {
        conn.execute("DELETE FROM edges WHERE src = ?1", [id])?;
        for edge in &edges {
            conn.execute(
                "INSERT INTO edges (src, dst, kind) VALUES (?1, ?2, ?3)",
                params![id, edge.dst.as_str(), text(&json!(edge.kind))],
            )?;
        }
        changed = true;
    }
    if tickets::comments(conn, id)? != whole.comments {
        conn.execute("DELETE FROM comments WHERE ticket = ?1", [id])?;
        for comment in &whole.comments {
            conn.execute(
                "INSERT INTO comments (ticket, n, author, at, text, legacy_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    comment.n,
                    comment.author,
                    comment.at.to_string(),
                    comment.text,
                    comment.legacy_id
                ],
            )?;
        }
        changed = true;
    }
    if tickets::history(conn, id)? != whole.history {
        conn.execute("DELETE FROM history WHERE ticket = ?1", [id])?;
        for history in &whole.history {
            conn.execute(
                "INSERT INTO history (ticket, seq, actor, at, change) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    history.seq.0 as i64,
                    history.actor,
                    history.at.to_string(),
                    history.change.to_string()
                ],
            )?;
        }
        changed = true;
    }
    for alias in &whole.aliases {
        changed |= conn.execute(
            "INSERT INTO aliases (alias, id) VALUES (?1, ?2) \
             ON CONFLICT(alias) DO UPDATE SET id = excluded.id WHERE id != excluded.id",
            params![alias, id],
        )? > 0;
    }
    Ok(changed)
}

/// Inserts or replaces a ticket row from the ticket's JSON, whose keys are the column names.
fn write_ticket(conn: &Connection, ticket: &Value) -> Result<()> {
    let mut statement = conn.prepare("SELECT name FROM pragma_table_info('tickets')")?;
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let values: Vec<SqlValue> = columns
        .iter()
        .map(|column| sql_value(column, ticket.get(column).unwrap_or(&Value::Null)))
        .collect();
    let updates: Vec<String> = columns
        .iter()
        .skip(1)
        .map(|column| format!("{column} = excluded.{column}"))
        .collect();
    let sql = format!(
        "INSERT INTO tickets ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
        columns.join(", "),
        vec!["?"; columns.len()].join(", "),
        updates.join(", ")
    );
    conn.execute(&sql, params_from_iter(values))?;
    Ok(())
}

fn sql_value(column: &str, value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        _ if JSON_COLUMNS.contains(&column) => SqlValue::Text(value.to_string()),
        Value::Bool(flag) => SqlValue::Integer(i64::from(*flag)),
        Value::Number(number) => number.as_i64().map_or_else(
            || SqlValue::Real(number.as_f64().unwrap_or_default()),
            SqlValue::Integer,
        ),
        Value::String(text) => SqlValue::Text(text.clone()),
        Value::Array(_) | Value::Object(_) => SqlValue::Text(value.to_string()),
    }
}

/// The text of a JSON string, such as a serialized enum.
fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// Refuses a page that leaves an imported ticket with a missing parent or in a parent cycle.
fn check_parents(conn: &Connection, page: &[WholeTicket]) -> Result<()> {
    let mut parents: BTreeMap<String, Option<String>> = BTreeMap::new();
    for whole in page {
        let mut current = Some(whole.ticket.id.as_str().to_owned());
        let mut path = HashSet::new();
        while let Some(id) = current {
            if !path.insert(id.clone()) {
                return Err(StoreError::Rejected(format!(
                    "Ticket {id} is its own ancestor"
                )));
            }
            let parent = if let Some(parent) = parents.get(&id) {
                parent.clone()
            } else {
                let ticket = tickets::load(conn, &id)?.ok_or_else(|| {
                    StoreError::Rejected(format!("Parent {id} of an imported ticket is missing"))
                })?;
                let parent = ticket.parent.map(|p| p.as_str().to_owned());
                parents.insert(id, parent.clone());
                parent
            };
            current = parent;
        }
    }
    Ok(())
}
