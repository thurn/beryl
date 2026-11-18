//! The tip verifier's memory as one JSON row, so that a restarted daemon goes on from what the
//! last round reported and held off.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::local::rows;

/// The saved memory, or `None` when none was ever saved.
pub fn get(conn: &Connection) -> Result<Option<rows::TipMemory>> {
    Ok(conn
        .query_row(
            "SELECT id, body, updated_at FROM tip_state WHERE id = 1",
            [],
            |row| {
                Ok(rows::TipMemory {
                    id: row.get(0)?,
                    body: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            },
        )
        .optional()?)
}

/// Replaces the saved memory with `body`.
pub fn put(conn: &Connection, body: &str, updated_at: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO tip_state (id, body, updated_at) VALUES (1, ?1, ?2)",
        params![body, updated_at],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn the_memory_is_one_row_that_is_replaced() {
        let conn = open_in_memory().unwrap();
        assert_eq!(get(&conn).unwrap(), None);
        put(&conn, "{\"a\":1}", "t1").unwrap();
        put(&conn, "{\"a\":2}", "t2").unwrap();
        let row = get(&conn).unwrap().unwrap();
        assert_eq!(
            (row.body.as_str(), row.updated_at.as_str()),
            ("{\"a\":2}", "t2")
        );
    }
}
