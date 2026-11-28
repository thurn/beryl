//! The emergency pause (see `ignite_core::protocol::emergency`), kept as one JSON value in the
//! `meta` table, so it needs no table of its own.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::protocol::emergency::{
    self, Emergency, EmergencyChange, EmergencyScope, has_emergency_label,
};
use ignite_core::protocol::tickets::Status;
use ignite_core::time::Timestamp;

use crate::{Result, StoreError};

use super::tickets;

/// The `meta` key that holds the emergency.
const KEY: &str = "emergency";

/// The emergency stored, whether or not it has lapsed.
pub fn stored(conn: &Connection) -> Result<Option<Emergency>> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [KEY], |row| {
            row.get(0)
        })
        .optional()?;
    value
        .map(|json| {
            serde_json::from_str(&json)
                .map_err(|e| StoreError::Invalid(format!("the stored emergency: {e}")))
        })
        .transpose()
}

/// The emergency in force at `now`: none once it has lapsed.
pub fn in_force(conn: &Connection, now: Timestamp) -> Result<Option<Emergency>> {
    Ok(stored(conn)?.filter(|e| e.active(now)))
}

/// The active emergency that pauses worker hand-offs, if any.
pub fn dispatch_hold(conn: &Connection, now: Timestamp) -> Result<Option<Emergency>> {
    Ok(in_force(conn, now)?.filter(|e| e.scope == EmergencyScope::All))
}

/// Applies `change` at `now` and returns the emergency it leaves, `None` once it has ended. Only
/// the session holding an incident or justiciar ticket may declare an emergency for it, or adopt
/// one onto it. A change the rules refuse is [`StoreError::Rejected`].
pub fn change(
    conn: &Connection,
    change: &EmergencyChange,
    now: Timestamp,
) -> Result<Option<Emergency>> {
    let tx = conn.unchecked_transaction()?;
    let claim = match change {
        EmergencyChange::Declare {
            ticket,
            session,
            machine,
            ..
        }
        | EmergencyChange::Adopt {
            ticket,
            session,
            machine,
        } => Some((ticket, session, machine)),
        EmergencyChange::Allow { .. } | EmergencyChange::End { .. } => None,
    };
    if let Some((ticket, session, machine)) = claim
        && !tickets::load(&tx, ticket.as_str())?.is_some_and(|held| {
            held.status == Status::InProgress
                && held.session.as_ref() == Some(session)
                && held.machine.as_ref() == Some(machine)
                && has_emergency_label(&held.labels)
        })
    {
        let what = match change {
            EmergencyChange::Adopt { .. } => "adopt an emergency onto it",
            _ => "declare an emergency for it",
        };
        return Err(StoreError::Rejected(format!(
            "only a session holding an in-progress incident or justiciar ticket may {what}. \
             {session} does not hold such a claim on {ticket} from {machine}"
        )));
    }
    let current = stored(&tx)?;
    // Another session may have declared an emergency since the caller declared its own, so
    // only holding the emergency's own ticket lets a caller end it unnamed.
    let holds = match (change, &current) {
        (
            EmergencyChange::End {
                session: Some(session),
                ..
            },
            Some(current),
        ) => tickets::load(&tx, current.ticket.as_str())?.is_some_and(|held| {
            held.status == Status::InProgress && held.session.as_ref() == Some(session)
        }),
        _ => false,
    };
    let next = emergency::apply(current, change, now, holds).map_err(StoreError::Rejected)?;
    match &next {
        Some(next) => write(&tx, next)?,
        None => clear(&tx)?,
    }
    tx.commit()?;
    Ok(next)
}

/// Stores `emergency` as given, without the rule of [`change`] that only a session holding an
/// incident or justiciar ticket may declare. Hub upkeep uses it to freeze landings by itself
/// when `master` falls far behind, since no session declares that emergency.
pub fn impose(conn: &Connection, emergency: &Emergency) -> Result<()> {
    write(conn, emergency)
}

fn write(conn: &Connection, emergency: &Emergency) -> Result<()> {
    let json = serde_json::to_string(emergency)
        .map_err(|e| StoreError::Invalid(format!("emergency: {e}")))?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY, json],
    )?;
    Ok(())
}

/// Ends the emergency, as `ignite emergency end` does.
pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM meta WHERE key = ?1", [KEY])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;
    use ignite_core::ids::{MachineId, SessionId, TicketId};
    use ignite_core::protocol::emergency::EMERGENCY_LASTS_S;

    #[test]
    fn an_emergency_is_kept_until_it_lapses_or_is_cleared() {
        let conn = open_in_memory().unwrap();
        let now = Timestamp::EPOCH;
        assert_eq!(in_force(&conn, now).unwrap(), None);
        conn.execute(
            "INSERT INTO tickets
                (id, title, status, labels, session, machine, created_at, created_by, updated_at, version)
             VALUES ('IP4R7', 'Incident', 'in_progress', '[\"incident\"]', 'local_i', 'm-1',
                     '1970-01-01T00:00:00Z', 'test', '1970-01-01T00:00:00Z', 1)",
            [],
        ).unwrap();
        let declare = EmergencyChange::Declare {
            ticket: TicketId::new("IP4R7").unwrap(),
            session: SessionId::new("local_i").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            scope: EmergencyScope::All,
        };
        let declared = change(&conn, &declare, now).unwrap();
        assert_eq!(in_force(&conn, now).unwrap(), declared);
        let lapsed = now.plus_seconds(EMERGENCY_LASTS_S);
        assert_eq!(in_force(&conn, lapsed).unwrap(), None);
        clear(&conn).unwrap();
        assert_eq!(stored(&conn).unwrap(), None);
    }
}
