//! Ticket owners at the hub (see `ignite_core::owner`): the routing facts from `ignite.toml`,
//! which the hub records at start, and the default owner, which a person changes on the
//! dashboard. Both are values in the `meta` table.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use ignite_core::config::Os;
use ignite_core::owner::{Owner, Routing};
use ignite_core::time::Timestamp;

use crate::{Result, StoreError};

/// The `meta` key of the routing facts from `ignite.toml`.
const ROUTING: &str = "routing";

/// The `meta` key of the default owner a person picked.
const DEFAULT_OWNER: &str = "default_owner";

/// The default owner a person picked, and who picked it when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultOwner {
    pub owner: Owner,
    pub by: String,
    pub at: Timestamp,
}

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

/// The default owner a person picked, if any.
pub fn picked_default(conn: &Connection) -> Result<Option<DefaultOwner>> {
    get(conn, DEFAULT_OWNER)
}

/// The default owner in force: the one a person picked, or else `[pool] default_owner`.
pub fn default_owner(conn: &Connection, routing: &Routing) -> Result<Option<Owner>> {
    Ok(picked_default(conn)?
        .map(|picked| picked.owner)
        .or_else(|| routing.default_owner.clone()))
}

/// Makes `owner` the default owner. It must be one of [`Routing::choices`].
pub fn set_default_owner(conn: &Connection, owner: &Owner, by: &str, now: Timestamp) -> Result<()> {
    let routing = routing(conn)?;
    if !routing.choices().contains(owner) {
        return Err(StoreError::Rejected(format!(
            "{owner} is not a machine and app of ignite.toml"
        )));
    }
    put(
        conn,
        DEFAULT_OWNER,
        &DefaultOwner {
            owner: owner.clone(),
            by: by.to_owned(),
            at: now,
        },
    )
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

    fn routing() -> Routing {
        Routing {
            machines: vec![RoutedMachine {
                name: "jarvis".to_owned(),
                os: Some(Os::Macos),
                home_app: Some("claude".to_owned()),
            }],
            apps: vec!["claude".to_owned(), "codex".to_owned()],
            default_owner: Some("jarvis/claude".parse().unwrap()),
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
    fn the_default_owner_is_the_configured_one_until_a_person_picks_another() {
        let conn = open_in_memory().unwrap();
        record_routing(&conn, &routing()).unwrap();
        let routing = super::routing(&conn).unwrap();
        assert_eq!(
            default_owner(&conn, &routing).unwrap(),
            Some("jarvis/claude".parse().unwrap())
        );
        let codex: Owner = "jarvis/codex".parse().unwrap();
        set_default_owner(&conn, &codex, "derek", Timestamp::from_millis(5)).unwrap();
        assert_eq!(default_owner(&conn, &routing).unwrap(), Some(codex));
        assert_eq!(picked_default(&conn).unwrap().unwrap().by, "derek");
        let unknown: Owner = "nowhere/claude".parse().unwrap();
        assert!(set_default_owner(&conn, &unknown, "derek", Timestamp::from_millis(6)).is_err());
    }
}
