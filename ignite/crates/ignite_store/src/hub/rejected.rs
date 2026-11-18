//! Rejected commands: command lines the `ignite` executable refused to parse. The
//! `rejected-command` improvement rule reads them to find the verbs and flags agents look for.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::protocol::hub::RejectedCommand;
use ignite_core::time::Timestamp;

use crate::hub::rows;
use crate::{Result, StoreError};

/// Stores one rejected command from `machine`, with the ticket its session holds, if any.
pub fn insert(
    conn: &Connection,
    machine: &str,
    command: &RejectedCommand,
    now: Timestamp,
) -> Result<()> {
    let session = command.session.as_ref().map(ToString::to_string);
    let ticket: Option<String> = match &session {
        Some(session) => conn
            .query_row(
                "SELECT id FROM tickets WHERE session = ?1 AND status = 'in_progress'
                 ORDER BY claimed_at DESC LIMIT 1",
                [session],
                |row| row.get(0),
            )
            .optional()?,
        None => None,
    };
    conn.execute(
        "INSERT INTO rejected_commands (machine, session, ticket, verb, args, kind, suggestion, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            machine,
            session,
            ticket,
            command.verb,
            serde_json::to_string(&command.args)
                .map_err(|error| StoreError::Invalid(error.to_string()))?,
            command.kind,
            command.suggestion,
            now.to_string(),
        ],
    )?;
    Ok(())
}

/// The rejected commands at or after `since` and at or before `until`, oldest first.
pub fn between(
    conn: &Connection,
    since: Timestamp,
    until: Timestamp,
) -> Result<Vec<rows::RejectedCommand>> {
    let mut statement = conn.prepare(
        "SELECT id, machine, session, ticket, verb, args, kind, suggestion, at
         FROM rejected_commands WHERE at >= ?1 AND at <= ?2 ORDER BY at, id",
    )?;
    let rows = statement
        .query_map(params![since.to_string(), until.to_string()], |row| {
            Ok(rows::RejectedCommand {
                id: row.get(0)?,
                machine: row.get(1)?,
                session: row.get(2)?,
                ticket: row.get(3)?,
                verb: row.get(4)?,
                args: row.get(5)?,
                kind: row.get(6)?,
                suggestion: row.get(7)?,
                at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn a_rejected_command_is_stored_with_the_ticket_its_session_holds() {
        let conn = hub::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO tickets (id, title, type, status, priority, labels, metadata,
                                  created_at, created_by, updated_at, version, session, claimed_at)
             VALUES ('a.1', 't', 'task', 'in_progress', 2, '[]', '{}', 'x', 'x', 'x', 1, 's-1', 'x')",
            [],
        )
        .unwrap();
        let command = RejectedCommand {
            verb: "ticket dep list".to_owned(),
            args: vec!["ticket".into(), "dep".into(), "list".into(), "a.1".into()],
            kind: "unknown subcommand".to_owned(),
            suggestion: None,
            session: Some("s-1".parse().unwrap()),
        };
        insert(&conn, "m-1", &command, at("2026-11-17T12:00:00.000Z")).unwrap();
        let rows = between(
            &conn,
            at("2026-11-17T00:00:00.000Z"),
            at("2026-11-18T00:00:00.000Z"),
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ticket.as_deref(), Some("a.1"));
        assert_eq!(rows[0].verb, "ticket dep list");
        assert_eq!(rows[0].args, r#"["ticket","dep","list","a.1"]"#);
    }
}
