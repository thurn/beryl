//! Ticket pins resolved independently through the ticket's ancestors.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension};

use ignite_core::pins::{Pins, from_labels};

use crate::{Result, StoreError};

/// Finds each pin on the ticket or its closest ancestor with a label of that kind.
pub fn of(conn: &Connection, ticket: &str) -> Result<Pins> {
    let mut pins = Pins::default();
    let mut seen = HashSet::new();
    let mut current = Some(ticket.to_owned());
    let mut lookup = conn.prepare_cached("SELECT parent, labels FROM tickets WHERE id = ?1")?;
    while let Some(id) = current.take() {
        let row: Option<(Option<String>, String)> = lookup
            .query_row([&id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        let Some((parent, labels)) = row else {
            break;
        };
        let labels: Vec<String> = serde_json::from_str(&labels)
            .map_err(|error| StoreError::Invalid(format!("labels on {id}: {error}")))?;
        let own = from_labels(&labels);
        pins.machine = pins.machine.or(own.machine);
        pins.provider = pins.provider.or(own.provider);
        if pins.machine.is_some() && pins.provider.is_some() || !seen.insert(id) {
            break;
        }
        current = parent;
    }
    Ok(pins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;
    use rusqlite::params;

    #[test]
    fn each_pin_comes_from_the_closest_matching_label() {
        let conn = open_in_memory().unwrap();
        for (id, parent, labels) in [
            ("T", None, r#"["machine:host","provider:claude"]"#),
            ("T.1", Some("T"), r#"["provider:codex"]"#),
            ("T.1.1", Some("T.1"), "[]"),
            ("T.1.2", Some("T.1"), r#"["machine:other"]"#),
        ] {
            conn.execute(
                "INSERT INTO tickets (id, parent, title, created_at, created_by, updated_at, version, labels) \
                 VALUES (?1, ?2, 'test', 'now', 'test', 'now', 0, ?3)",
                params![id, parent, labels],
            )
            .unwrap();
        }
        assert_eq!(
            of(&conn, "T.1.1").unwrap(),
            Pins {
                machine: Some("host".to_owned()),
                provider: Some("codex".to_owned()),
            }
        );
        assert_eq!(
            of(&conn, "T.1.2").unwrap().machine.as_deref(),
            Some("other")
        );
        assert_eq!(of(&conn, "missing").unwrap(), Pins::default());
    }
}
