//! The emergency pause (see `ignite_core::protocol::emergency`), kept as one JSON value in the
//! `meta` table, so it needs no table of its own.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::protocol::emergency::{self, Emergency, EmergencyChange};
use ignite_core::time::Timestamp;

use crate::{Result, StoreError};

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

/// Applies `change` at `now` and returns the emergency it leaves. A change the rules refuse,
/// such as an allowance from a session that is not the justiciar's, is
/// [`StoreError::Rejected`].
pub fn change(conn: &Connection, change: &EmergencyChange, now: Timestamp) -> Result<Emergency> {
    let next = emergency::apply(stored(conn)?, change, now).map_err(StoreError::Rejected)?;
    let json =
        serde_json::to_string(&next).map_err(|e| StoreError::Invalid(format!("emergency: {e}")))?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY, json],
    )?;
    Ok(next)
}

/// Ends the emergency, as `ignite pool resume --all` does.
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
        let declare = EmergencyChange::Declare {
            ticket: TicketId::new("TJUS1").unwrap(),
            session: SessionId::new("local_j").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
        };
        let declared = change(&conn, &declare, now).unwrap();
        assert_eq!(in_force(&conn, now).unwrap(), Some(declared));
        let lapsed = now.plus_seconds(EMERGENCY_LASTS_S);
        assert_eq!(in_force(&conn, lapsed).unwrap(), None);
        clear(&conn).unwrap();
        assert_eq!(stored(&conn).unwrap(), None);
    }
}
