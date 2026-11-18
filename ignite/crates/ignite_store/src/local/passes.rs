//! How long each check command took when it passed, from which the check runner computes the
//! command's hang limit.

use rusqlite::{Connection, params};

use crate::Result;

/// Records that `family`'s `command` (`list` or `run`) passed in `wall_s` seconds at `at`, and
/// forgets that command's passes from before `forget_before`.
pub fn add(
    conn: &Connection,
    family: &str,
    command: &str,
    wall_s: f64,
    at: &str,
    forget_before: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO passes (family, command, wall_s, at) VALUES (?1, ?2, ?3, ?4)",
        params![family, command, wall_s, at],
    )?;
    conn.execute(
        "DELETE FROM passes WHERE family = ?1 AND command = ?2 AND at < ?3",
        params![family, command, forget_before],
    )?;
    Ok(())
}

/// The longest pass of `family`'s `command` at or after `since`, if any.
pub fn slowest(conn: &Connection, family: &str, command: &str, since: &str) -> Result<Option<f64>> {
    Ok(conn.query_row(
        "SELECT MAX(wall_s) FROM passes WHERE family = ?1 AND command = ?2 AND at >= ?3",
        params![family, command, since],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn the_slowest_recent_pass_of_one_command() {
        let conn = open_in_memory().unwrap();
        add(&conn, "unit", "run", 90.0, "2026-11-01", "2026-10-01").unwrap();
        add(&conn, "unit", "run", 30.0, "2026-11-10", "2026-10-01").unwrap();
        add(&conn, "unit", "list", 500.0, "2026-11-10", "2026-10-01").unwrap();
        add(&conn, "other", "run", 700.0, "2026-11-10", "2026-10-01").unwrap();
        assert_eq!(
            slowest(&conn, "unit", "run", "2026-10-15").unwrap(),
            Some(90.0)
        );
        assert_eq!(
            slowest(&conn, "unit", "run", "2026-11-05").unwrap(),
            Some(30.0)
        );
        assert_eq!(slowest(&conn, "unit", "build", "2026-10-01").unwrap(), None);
        // A later pass forgets the old ones.
        add(&conn, "unit", "run", 10.0, "2026-12-20", "2026-11-20").unwrap();
        assert_eq!(
            slowest(&conn, "unit", "run", "2000-01-01").unwrap(),
            Some(10.0)
        );
    }
}
