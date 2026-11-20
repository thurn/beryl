//! Hierarchical ticket IDs. A child's ID is its parent's ID and one more dotted number
//! (`EME7A.22` is a child of `EME7A`), and a ticket with no parent has a root ID with no dots.
//! The hub keeps this true for every ticket: moving a ticket to another parent, or detaching it,
//! gives it and every ticket below it a new ID ([`subtree`] and [`apply`]). Each old ID becomes
//! an alias of the new one, so a name written before the move, in a commit message or another
//! ticket's text, still finds the ticket.
//!
//! [`renumber_all`] is the migration that brought tickets moved before this rule into line.

use std::collections::{BTreeMap, HashMap};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::json;

use ignite_core::ids::{Seq, TicketId};
use ignite_core::protocol::tickets::FeedChange;

use crate::hub::batch::roots;
use crate::hub::feed;
use crate::{Result, StoreError};

/// The parent ID that `id` implies: `id` without its last dotted part, or `None` for a root.
#[must_use]
pub fn parent_of(id: &str) -> Option<&str> {
    id.rsplit_once('.').map(|(parent, _)| parent)
}

/// Every column outside `tickets` that holds a ticket ID, as `(table, column)`. A column that
/// may also name something other than a ticket is rewritten only where it names a renamed one.
const ID_COLUMNS: &[(&str, &str)] = &[
    ("edges", "src"),
    ("edges", "dst"),
    ("comments", "ticket"),
    ("history", "ticket"),
    ("aliases", "id"),
    ("workers", "ticket"),
    ("reservations", "ticket"),
    ("handoffs", "ticket"),
    ("regressions", "ticket"),
    ("records", "ticket"),
    ("findings", "ticket"),
    ("feed", "ticket"),
    ("check_state_changes", "ticket"),
    ("rejected_commands", "ticket"),
    ("flaky", "filed"),
    ("ticket_spend", "ticket"),
    ("tip_misses", "ticket"),
];

/// The renames that give `old` the ID `new`: `old` itself, and each ticket below it with the
/// same dotted numbers after the new prefix (`TQ44S.113.2` becomes `EME7A.22.2`).
pub fn subtree(conn: &Connection, old: &str, new: &str) -> Result<Vec<(String, String)>> {
    let mut statement = conn
        .prepare("SELECT id FROM tickets WHERE id = ?1 OR instr(id, ?1 || '.') = 1 ORDER BY id")?;
    let ids = statement.query_map([old], |row| row.get::<_, String>(0))?;
    let mut renames = Vec::new();
    for id in ids {
        let id = id?;
        let rest = &id[old.len()..];
        renames.push((id.clone(), format!("{new}{rest}")));
    }
    Ok(renames)
}

/// Gives each ticket in `renames` (old ID, new ID) its new ID, sets its parent to the one the
/// new ID implies, rewrites every other table that names it, and records the old ID as an alias.
/// Refuses a new ID that a ticket or an alias already uses. Ticket text that mentions an old ID
/// is left as written: the alias resolves it.
pub fn apply(conn: &Connection, renames: &[(String, String)]) -> Result<()> {
    if renames.is_empty() {
        return Ok(());
    }
    for (_, new) in renames {
        let used: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM tickets WHERE id = ?1 \
                 UNION ALL SELECT 1 FROM aliases WHERE alias = lower(?1) LIMIT 1",
                [new],
                |row| row.get(0),
            )
            .optional()?;
        if used.is_some() {
            return Err(StoreError::Invalid(format!(
                "ticket ID {new} is already in use"
            )));
        }
    }
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS renames (old TEXT PRIMARY KEY, new TEXT NOT NULL); \
         DELETE FROM temp.renames;",
    )?;
    for (old, new) in renames {
        conn.execute(
            "INSERT INTO temp.renames (old, new) VALUES (?1, ?2)",
            [old, new],
        )?;
    }
    for (table, column) in ID_COLUMNS {
        conn.execute(
            &format!(
                "UPDATE {table} SET {column} = \
                   (SELECT new FROM temp.renames WHERE old = {table}.{column}) \
                 WHERE {column} IN (SELECT old FROM temp.renames)"
            ),
            [],
        )?;
    }
    // A create request's key, kept for a day so that a retried request returns its ticket.
    conn.execute(
        "UPDATE meta SET value = json_set(value, '$.id', \
           (SELECT new FROM temp.renames WHERE old = meta.value ->> '$.id')) \
         WHERE key GLOB 'create:*' AND value ->> '$.id' IN (SELECT old FROM temp.renames)",
        [],
    )?;
    for (old, new) in renames {
        conn.execute(
            "UPDATE tickets SET id = ?1, parent = ?2 WHERE id = ?3",
            params![new, parent_of(new), old],
        )?;
        // Aliases are stored in lowercase, as lookups compare them.
        conn.execute(
            "INSERT INTO aliases (alias, id) VALUES (lower(?1), ?2) \
             ON CONFLICT(alias) DO UPDATE SET id = excluded.id",
            [old, new],
        )?;
    }
    conn.execute("DELETE FROM temp.renames", [])?;
    Ok(())
}

/// The tickets whose parent is not the one their ID implies, as `(id, parent)`.
pub fn violations(conn: &Connection) -> Result<Vec<(String, Option<String>)>> {
    let mut statement = conn.prepare("SELECT id, parent FROM tickets ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut found = Vec::new();
    for row in rows {
        let (id, parent) = row?;
        if parent_of(&id) != parent.as_deref() {
            found.push((id, parent));
        }
    }
    Ok(found)
}

/// One ticket as the migration reads it.
struct Row {
    parent: Option<String>,
    next_child: u32,
    epic: bool,
}

/// The migration that gives every ticket moved before IDs were kept hierarchical the ID its
/// place in the tree implies. Walking down from each root, a child whose ID already extends its
/// parent's old ID keeps its own number under the parent's new ID; any other child takes its
/// parent's next free number. A ticket with dots in its ID and no parent, or whose parent no
/// longer exists, gets a new root ID. Each renamed ticket gets a changelog entry and a feed
/// change, so every machine's ticket mirror fetches it under its new ID.
pub fn renumber_all(transaction: &Transaction<'_>) -> Result<()> {
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    {
        let mut statement =
            transaction.prepare("SELECT id, parent, next_child, type FROM tickets")?;
        let found = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Row {
                    parent: row.get(1)?,
                    next_child: row.get(2)?,
                    epic: row.get::<_, String>(3)? == "epic",
                },
            ))
        })?;
        for row in found {
            let (id, row) = row?;
            rows.insert(id, row);
        }
    }
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut roots = Vec::new();
    for (id, row) in &rows {
        match &row.parent {
            Some(parent) if rows.contains_key(parent) => {
                children.entry(parent.clone()).or_default().push(id.clone());
            }
            _ => roots.push(id.clone()),
        }
    }
    let mut new_ids: HashMap<String, String> = HashMap::new();
    let mut next_child: BTreeMap<String, u32> = BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();
    for root in roots {
        let new = if root.contains('.') {
            let letter = root_letter(&root, rows[&root].epic);
            roots::allocate(transaction, letter)?
        } else {
            root.clone()
        };
        new_ids.insert(root.clone(), new);
        stack.push(root);
    }
    while let Some(parent) = stack.pop() {
        let new_parent = new_ids[&parent].clone();
        let below = children.remove(&parent).unwrap_or_default();
        // A number a child already holds stays taken, so a moved child never collides with it.
        let mut next = below
            .iter()
            .filter(|child| parent_of(child) == Some(parent.as_str()))
            .filter_map(|child| child.rsplit_once('.')?.1.parse::<u32>().ok())
            .map(|n| n + 1)
            .fold(rows[&parent].next_child, u32::max);
        for child in below {
            let new = if parent_of(&child) == Some(parent.as_str()) {
                format!("{new_parent}{}", &child[parent.len()..])
            } else {
                let number = next;
                next += 1;
                format!("{new_parent}.{number}")
            };
            new_ids.insert(child.clone(), new);
            stack.push(child);
        }
        if next != rows[&parent].next_child {
            next_child.insert(new_parent, next);
        }
    }
    let mut renames: Vec<(String, String)> = new_ids
        .into_iter()
        .filter(|(old, new)| old != new)
        .collect();
    renames.sort();
    // Tickets whose parent is missing keep their root ID but lose the parent.
    transaction.execute(
        "UPDATE tickets SET parent = NULL WHERE parent IS NOT NULL \
         AND parent NOT IN (SELECT id FROM tickets)",
        [],
    )?;
    apply(transaction, &renames)?;
    for (id, next) in &next_child {
        transaction.execute(
            "UPDATE tickets SET next_child = ?1 WHERE id = ?2",
            params![next, id],
        )?;
    }
    if !renames.is_empty() {
        record_migration(transaction, &renames)?;
    }
    let left = violations(transaction)?;
    if let Some((id, parent)) = left.first() {
        return Err(StoreError::Invalid(format!(
            "ticket {id} still has parent {parent:?} after renumbering"
        )));
    }
    Ok(())
}

/// The letter of a new root ID for a ticket detached from its tree: `E` for an epic, `A` when
/// its old ID started with `A` (a ticket Ignite filed on its own) or `B` (a bug report), and `T`
/// otherwise.
#[must_use]
pub fn root_letter(old: &str, epic: bool) -> char {
    if epic {
        'E'
    } else if old.starts_with('A') {
        'A'
    } else if old.starts_with('B') {
        'B'
    } else {
        'T'
    }
}

/// Writes one changelog entry per renamed ticket and a feed change at one new position.
fn record_migration(transaction: &Transaction<'_>, renames: &[(String, String)]) -> Result<()> {
    let seq = Seq(feed::current(transaction)?.0 + 1);
    feed::set_current(transaction, seq)?;
    let at: String =
        transaction.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
            row.get(0)
        })?;
    for (old, new) in renames {
        transaction.execute(
            "UPDATE tickets SET version = ?1 WHERE id = ?2",
            params![seq.0 as i64, new],
        )?;
        transaction.execute(
            "INSERT INTO history (ticket, seq, actor, at, change) VALUES (?1, ?2, 'ignite', ?3, ?4)",
            params![
                new,
                seq.0 as i64,
                at,
                json!({"id": [old, new]}).to_string()
            ],
        )?;
        let ticket: TicketId = new
            .parse()
            .map_err(|e| StoreError::Invalid(format!("ticket ID: {e}")))?;
        feed::record(
            transaction,
            &FeedChange {
                seq,
                kind: "ticket".to_owned(),
                ticket: Some(ticket),
                name: None,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "renumber_tests.rs"]
mod tests;
