//! Ticket owners at the hub (see `ignite_core::owner`): the routing facts from `ignite.toml`,
//! which the hub records at start as a value in the `meta` table, the owner of a ticket a
//! program files, and the move of the old routing labels into the ticket's fields.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde_json::Value;

use ignite_core::config::Os;
use ignite_core::owner::{Owner, Routing};
use ignite_core::protocol::tickets::{BatchRequest, Operation};

use crate::{Result, StoreError};

/// The `meta` key of the routing facts from `ignite.toml`.
const ROUTING: &str = "routing";

fn get<T: serde::de::DeserializeOwned>(conn: &Connection, key: &str) -> Result<Option<T>> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    value
        .map(|json| {
            serde_json::from_str(&json).map_err(|e| StoreError::Invalid(format!("meta {key}: {e}")))
        })
        .transpose()
}

fn put(conn: &Connection, key: &str, value: &impl Serialize) -> Result<()> {
    let json = serde_json::to_string(value)
        .map_err(|e| StoreError::Invalid(format!("meta {key}: {e}")))?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, json],
    )?;
    Ok(())
}

/// Records the routing facts of the hub's `ignite.toml`, replacing earlier ones.
pub fn record_routing(conn: &Connection, routing: &Routing) -> Result<()> {
    put(conn, ROUTING, routing)
}

/// The routing facts last recorded, or none for a hub that never recorded them.
pub fn routing(conn: &Connection) -> Result<Routing> {
    Ok(get(conn, ROUTING)?.unwrap_or_default())
}

/// Makes `machine`, the name of the machine writing, the owner of each ticket that `batch`
/// creates without naming an owner, for a batch a program filed (`ignite ticket create
/// --automatic`): an automatic filing stays on the machine that filed it, whatever app runs
/// there. A machine the hub has no name for, given as `None`, changes nothing, which leaves
/// the tickets to the default owner.
pub fn add_filing_machine(batch: &mut BatchRequest, machine: Option<&str>) {
    let Some(machine) = machine else {
        return;
    };
    for operation in &mut batch.ops {
        if let Operation::Create { fields, .. } = operation
            && fields.owner.is_none()
        {
            fields.owner = Some(Some(Owner::new(machine, None)));
        }
    }
}

/// Moves the routing labels into the ticket's own fields: `needs:macos` and `needs:windows`
/// become `needs_os`, `needs:<machine>` for a registered machine becomes the owner, and
/// `provider:` labels, which named an app without a machine, are dropped. `needs:person` stays.
pub fn retire_labels(transaction: &Transaction<'_>) -> Result<()> {
    let machines: Vec<String> = {
        let mut statement = transaction.prepare("SELECT DISTINCT lower(name) FROM machines")?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let rows: Vec<(String, String)> = {
        let mut statement = transaction.prepare(
            "SELECT id, labels FROM tickets \
             WHERE labels LIKE '%\"needs:%' OR labels LIKE '%\"provider:%'",
        )?;
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, labels) in rows {
        let labels: Vec<String> = serde_json::from_str(&labels)
            .map_err(|e| StoreError::Invalid(format!("labels of {id}: {e}")))?;
        let mut kept = Vec::new();
        let mut owner: Option<String> = None;
        let mut needs_os: Option<String> = None;
        for label in labels {
            if label.starts_with("provider:") {
                continue;
            }
            match label.strip_prefix("needs:") {
                Some(os) if os_named(os).is_some() => needs_os = Some(os.to_owned()),
                Some(name) if machines.iter().any(|m| m.eq_ignore_ascii_case(name)) => {
                    owner = Some(name.to_ascii_lowercase());
                }
                _ => kept.push(label),
            }
        }
        transaction.execute(
            "UPDATE tickets SET labels = ?2, owner = COALESCE(owner, ?3), \
             needs_os = COALESCE(needs_os, ?4) WHERE id = ?1",
            params![id, Value::from(kept).to_string(), owner, needs_os],
        )?;
    }
    Ok(())
}

/// The operating system a word names, such as `macos`.
fn os_named(word: &str) -> Option<Os> {
    serde_json::from_value(Value::String(word.to_owned())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;
    use ignite_core::owner::RoutedMachine;
    use ignite_core::protocol::tickets::TicketFields;

    fn routing() -> Routing {
        Routing {
            machines: vec![RoutedMachine {
                name: "jarvis".to_owned(),
                os: Some(Os::Macos),
            }],
            apps: vec!["claude".to_owned(), "codex".to_owned()],
            default_owner: Some(Owner::new("jarvis", Some("claude"))),
        }
    }

    #[test]
    fn routing_labels_move_into_the_ticket_fields() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        let before = &crate::hub::MIGRATIONS[..13];
        crate::migrate::apply(&mut conn, before).unwrap();
        conn.execute_batch(
            "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
               VALUES ('m1', 'Cygnus', 'windows', 8, 1, '[]', 'v', '2026-01-01T00:00:00Z');
             INSERT INTO tickets (id, title, created_at, created_by, updated_at, version, labels)
               VALUES ('T1', 'a', 'x', 'x', 'x', 1, '[\"needs:windows\",\"needs:cygnus\",\"tier:opus\"]'),
                      ('T2', 'b', 'x', 'x', 'x', 1, '[\"needs:person\",\"provider:codex\"]');",
        )
        .unwrap();
        crate::migrate::apply(&mut conn, crate::hub::MIGRATIONS).unwrap();
        let row = |id: &str| -> (String, Option<String>, Option<String>) {
            conn.query_row(
                "SELECT labels, owner, needs_os FROM tickets WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!(
            row("T1"),
            (
                "[\"tier:opus\"]".to_owned(),
                Some("cygnus".to_owned()),
                Some("windows".to_owned())
            )
        );
        assert_eq!(row("T2"), ("[\"needs:person\"]".to_owned(), None, None));
    }

    #[test]
    fn the_routing_facts_read_back_as_recorded() {
        let conn = open_in_memory().unwrap();
        assert_eq!(super::routing(&conn).unwrap(), Routing::default());
        record_routing(&conn, &routing()).unwrap();
        assert_eq!(super::routing(&conn).unwrap(), routing());
    }

    #[test]
    fn an_automatic_filing_is_owned_by_the_machine_that_filed_it() {
        let create = |owner: Option<&str>| Operation::Create {
            fields: TicketFields {
                title: Some("Count rose".to_owned()),
                owner: owner.map(|text| Some(text.parse().unwrap())),
                ..TicketFields::default()
            },
            parent: None,
            deps: Vec::new(),
            request_key: None,
        };
        let owners_of = |machine: Option<&str>| {
            let mut batch = BatchRequest {
                preconditions: Vec::new(),
                ops: vec![create(None), create(Some("jarvis/codex"))],
            };
            add_filing_machine(&mut batch, machine);
            batch
                .ops
                .iter()
                .map(|op| match op {
                    Operation::Create { fields, .. } => fields
                        .owner
                        .clone()
                        .flatten()
                        .map_or_else(|| "-".to_owned(), |owner| owner.to_string()),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
        };
        // Named owners are kept; the rest go to the filing machine, with any app.
        assert_eq!(owners_of(Some("Cygnus")), ["cygnus", "jarvis/codex"]);
        // A machine the hub cannot name leaves them to the default owner.
        assert_eq!(owners_of(None), ["-", "jarvis/codex"]);
    }
}
