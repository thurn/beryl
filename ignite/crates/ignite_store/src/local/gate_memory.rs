//! What the landing gate remembers from one gate to the next: the checks each change's last
//! passing gate chose by score, and the rate the gates' check commands achieved lately. Each
//! long verb runs in a process of its own, so the memory lives here, where the next gate's
//! process finds it.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// The checks the last passing gate of `change` chose by score, or `None` when no gate of it
/// is remembered.
pub fn chosen(conn: &Connection, change: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT checks FROM gate_choices WHERE change = ?1",
            [change],
            |row| row.get(0),
        )
        .optional()?)
}

/// Remembers `checks` as what the last passing gate of `change` chose, at `at` milliseconds,
/// and forgets all but the `keep` changes recorded last.
pub fn record_chosen(
    conn: &Connection,
    change: &str,
    checks: &str,
    at: i64,
    keep: usize,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO gate_choices (change, checks, at) VALUES (?1, ?2, ?3)",
        params![change, checks, at],
    )?;
    conn.execute(
        "DELETE FROM gate_choices WHERE change NOT IN \
         (SELECT change FROM gate_choices ORDER BY at DESC, change LIMIT ?1)",
        [i64::try_from(keep).unwrap_or(i64::MAX)],
    )?;
    Ok(())
}

/// The last rate recorded, in billions of instructions per second, with when it was recorded.
pub fn rate(conn: &Connection) -> Result<Option<(i64, f64)>> {
    Ok(conn
        .query_row("SELECT at, rate FROM gate_rate WHERE id = 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?)
}

/// Records `rate` as the rate at `at` milliseconds, in place of the one before.
pub fn set_rate(conn: &Connection, at: i64, rate: f64) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO gate_rate (id, at, rate) VALUES (1, ?1, ?2)",
        params![at, rate],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn a_change_keeps_its_last_choice_and_old_changes_are_forgotten() {
        let conn = open_in_memory().unwrap();
        assert_eq!(chosen(&conn, "a").unwrap(), None);
        record_chosen(&conn, "a", "x", 1, 2).unwrap();
        record_chosen(&conn, "a", "y", 2, 2).unwrap();
        assert_eq!(chosen(&conn, "a").unwrap().as_deref(), Some("y"));
        record_chosen(&conn, "b", "z", 3, 2).unwrap();
        record_chosen(&conn, "c", "w", 4, 2).unwrap();
        assert_eq!(chosen(&conn, "a").unwrap(), None);
        assert_eq!(chosen(&conn, "b").unwrap().as_deref(), Some("z"));
        assert_eq!(chosen(&conn, "c").unwrap().as_deref(), Some("w"));
    }

    #[test]
    fn the_rate_holds_one_value() {
        let conn = open_in_memory().unwrap();
        assert_eq!(rate(&conn).unwrap(), None);
        set_rate(&conn, 1, 2.5).unwrap();
        set_rate(&conn, 2, 3.5).unwrap();
        assert_eq!(rate(&conn).unwrap(), Some((2, 3.5)));
    }
}
