//! Data migrations for the ticket routing fields.

use rusqlite::{Transaction, params};
use serde_json::Value;

use ignite_core::config::Os;
use ignite_core::pins::{from_labels, machine_label};

use crate::{Result, StoreError};

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

/// Converts stored owners into hard pins. Explicit pins of either kind take precedence.
pub(super) fn owners_to_pins(transaction: &Transaction<'_>) -> Result<()> {
    let rows = {
        let mut statement =
            transaction.prepare("SELECT id, owner, labels FROM tickets WHERE owner IS NOT NULL")?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, owner, labels) in rows {
        let mut labels: Vec<String> = serde_json::from_str(&labels)
            .map_err(|e| StoreError::Invalid(format!("labels of {id}: {e}")))?;
        let pins = from_labels(&labels);
        let (machine, provider) = owner.split_once('/').unwrap_or((&owner, ""));
        if pins.machine.is_none() && !machine.is_empty() {
            labels.push(machine_label(machine));
        }
        if pins.provider.is_none() && !provider.is_empty() {
            labels.push(format!("provider:{}", provider.to_ascii_lowercase()));
        }
        transaction.execute(
            "UPDATE tickets SET labels = ?2 WHERE id = ?1",
            params![id, Value::from(labels).to_string()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        crate::migrate::apply(&mut conn, &crate::hub::MIGRATIONS[..14]).unwrap();
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
    fn stored_owners_become_pins_once_without_replacing_explicit_pins() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        let migrations = crate::hub::MIGRATIONS;
        crate::migrate::apply(&mut conn, &migrations[..33]).unwrap();
        for (id, owner, labels) in [
            ("T1", Some("jarvis/codex"), r#"["tier:opus"]"#),
            ("T2", Some("cygnus"), "[]"),
            ("T3", None, r#"["provider:codex"]"#),
            (
                "T4",
                Some("jarvis/codex"),
                r#"["machine:cygnus","provider:claude"]"#,
            ),
            ("T5", None, "[]"),
        ] {
            conn.execute(
                "INSERT INTO tickets (id, title, created_at, created_by, updated_at, version, owner, labels)
                 VALUES (?1, 'test', 'now', 'test', 'now', 0, ?2, ?3)",
                rusqlite::params![id, owner, labels],
            ).unwrap();
        }
        for _ in 0..2 {
            crate::migrate::apply(&mut conn, migrations).unwrap();
        }
        for (id, machine, provider) in [
            ("T1", Some("jarvis"), Some("codex")),
            ("T2", Some("cygnus"), None),
            ("T3", None, Some("codex")),
            ("T4", Some("cygnus"), Some("claude")),
            ("T5", None, None),
        ] {
            let pins = crate::hub::pins::of(&conn, id).unwrap();
            assert_eq!(pins.machine.as_deref(), machine);
            assert_eq!(pins.provider.as_deref(), provider);
        }
    }
}
