//! The stop state of verbs that run in processes of their own: whether a stop was asked, and
//! the reason stopping is refused while it is.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::local::rows;

pub fn get(conn: &Connection, handle: &str) -> Result<Option<rows::HandleStop>> {
    Ok(conn
        .query_row(
            "SELECT handle, asked, refused, at FROM handle_stops WHERE handle = ?1",
            [handle],
            |row| {
                Ok(rows::HandleStop {
                    handle: row.get(0)?,
                    asked: row.get(1)?,
                    refused: row.get(2)?,
                    at: row.get(3)?,
                })
            },
        )
        .optional()?)
}

/// Records the handle's stop state, replacing what was recorded.
pub fn set(conn: &Connection, stop: &rows::HandleStop) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO handle_stops (handle, asked, refused, at) VALUES (?1, ?2, ?3, ?4)",
        params![stop.handle, stop.asked, stop.refused, stop.at],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn a_stop_state_is_replaced_whole() {
        let conn = open_in_memory().unwrap();
        assert_eq!(get(&conn, "L-6").unwrap(), None);
        let refused = rows::HandleStop {
            handle: "L-6".to_owned(),
            asked: false,
            refused: Some("pushing to staging".to_owned()),
            at: "t1".to_owned(),
        };
        set(&conn, &refused).unwrap();
        assert_eq!(get(&conn, "L-6").unwrap(), Some(refused));
        let asked = rows::HandleStop {
            handle: "L-6".to_owned(),
            asked: true,
            refused: None,
            at: "t2".to_owned(),
        };
        set(&conn, &asked).unwrap();
        assert_eq!(get(&conn, "L-6").unwrap(), Some(asked));
    }
}
