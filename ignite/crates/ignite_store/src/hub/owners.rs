//! Ticket owners at the hub (see `ignite_core::owner`): the routing facts from `ignite.toml`,
//! which the hub records at start as a value in the `meta` table, and the owner a new ticket
//! takes from the session that files it.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde_json::Value;

use ignite_core::config::Os;
use ignite_core::owner::{Owner, Routing};
use ignite_core::protocol::tickets::{BatchRequest, Operation};

use crate::hub::tickets;
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

/// Makes `creator`, the machine and app of the session writing, the owner of each ticket that
/// `batch` creates without an owner whose ancestors have none either. A ticket under an owned
/// ancestor is left to inherit, so that changing the ancestor's owner moves it too. A parent
/// created in the same batch (`$<index>`) is owned by then, by this pass or by its own field.
/// A create that names its owner, or clears it with `null`, is kept.
pub fn add_creator_owners(
    conn: &Connection,
    batch: &mut BatchRequest,
    creator: Option<&Owner>,
) -> Result<()> {
    let Some(creator) = creator else {
        return Ok(());
    };
    for operation in &mut batch.ops {
        let Operation::Create { fields, parent, .. } = operation else {
            continue;
        };
        if fields.owner.is_some() {
            continue;
        }
        let parent_name = parent
            .clone()
            .or_else(|| fields.parent.clone().flatten().map(String::from));
        let inherits = match parent_name.as_deref() {
            None => false,
            Some(name) if name.starts_with('$') => true,
            Some(name) => owned_line(conn, name)?,
        };
        if !inherits {
            fields.owner = Some(Some(creator.clone()));
        }
    }
    Ok(())
}

/// Whether the ticket `name` names, or one of its ancestors, has an owner.
fn owned_line(conn: &Connection, name: &str) -> Result<bool> {
    let mut next = tickets::resolve(conn, name)?.map(|id| id.to_string());
    let mut seen = HashSet::new();
    while let Some(id) = next {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(ticket) = tickets::load(conn, &id)? else {
            break;
        };
        if ticket.owner.is_some() {
            return Ok(true);
        }
        next = ticket.parent.map(|parent| parent.to_string());
    }
    Ok(false)
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
    use crate::hub::batch::{self, BatchOutcome};
    use crate::hub::open_in_memory;
    use ignite_core::owner::RoutedMachine;
    use ignite_core::protocol::tickets::TicketFields;
    use ignite_core::time::Timestamp;

    fn routing() -> Routing {
        Routing {
            machines: vec![RoutedMachine {
                name: "jarvis".to_owned(),
                os: Some(Os::Macos),
            }],
            apps: vec!["claude".to_owned(), "codex".to_owned()],
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

    /// A create of `title`, naming `owner` when it is given; `none` clears the owner.
    fn create(title: &str, parent: Option<&str>, owner: Option<&str>) -> Operation {
        Operation::Create {
            fields: TicketFields {
                title: Some(title.to_owned()),
                owner: owner.map(|text| (text != "none").then(|| text.parse().unwrap())),
                ..TicketFields::default()
            },
            parent: parent.map(str::to_owned),
            deps: Vec::new(),
            request_key: None,
        }
    }

    /// The owner each create of `ops` ends up with after [`add_creator_owners`].
    fn stamped(conn: &Connection, ops: Vec<Operation>, creator: Option<&Owner>) -> Vec<String> {
        let mut request = BatchRequest {
            preconditions: Vec::new(),
            ops,
        };
        add_creator_owners(conn, &mut request, creator).unwrap();
        request
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
            .collect()
    }

    #[test]
    fn a_new_ticket_is_owned_by_its_creator_unless_it_inherits_or_names_one() {
        let mut conn = open_in_memory().unwrap();
        let creator: Owner = "multivac/claude".parse().unwrap();
        let by = Some(&creator);
        // A ticket on its own, and an epic whose child is filed in the same batch.
        let ops = vec![
            create("Alone", None, None),
            create("Epic", None, None),
            create("Child", Some("$1"), None),
        ];
        assert_eq!(
            stamped(&conn, ops, by),
            ["multivac/claude", "multivac/claude", "-"]
        );
        // An owner the create names, or clears, is kept.
        let ops = vec![
            create("Named", None, Some("jarvis/codex")),
            create("Cleared", None, Some("none")),
        ];
        assert_eq!(stamped(&conn, ops, by), ["jarvis/codex", "-"]);
        // Under an existing ticket it inherits an ancestor's owner, if one has an owner.
        let request = BatchRequest {
            preconditions: Vec::new(),
            ops: vec![
                create("Owned epic", None, Some("jarvis/claude")),
                create("Bare epic", None, None),
            ],
        };
        let Ok(BatchOutcome::Applied(reply)) =
            batch::apply(&mut conn, &request, "s", "m", Timestamp::from_millis(1))
        else {
            panic!("the epics were not filed");
        };
        let owned = reply.results[0].id.to_string();
        let bare = reply.results[1].id.to_string();
        let ops = vec![
            create("Under owned", Some(&owned), None),
            create("Under bare", Some(&bare), None),
        ];
        assert_eq!(stamped(&conn, ops, by), ["-", "multivac/claude"]);
        // Without a known creator nothing changes.
        assert_eq!(
            stamped(&conn, vec![create("Unknown", None, None)], None),
            ["-"]
        );
    }
}
