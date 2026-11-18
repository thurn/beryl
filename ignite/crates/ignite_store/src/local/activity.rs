//! When each held ticket tree's session last ran a command and how far the stall procedure has
//! gone, so that a restarted daemon carries on the 30-minute silence clock instead of starting
//! it over.

use rusqlite::{Connection, params};

use crate::Result;
use crate::local::rows;

/// Every recorded row.
pub fn all(conn: &Connection) -> Result<Vec<rows::ClaimActivity>> {
    let mut statement = conn.prepare(
        "SELECT tree, ticket, active_at, stage, checked_at, messaged_at, second \
         FROM claim_activity ORDER BY tree",
    )?;
    let found = statement
        .query_map([], |row| {
            Ok(rows::ClaimActivity {
                tree: row.get(0)?,
                ticket: row.get(1)?,
                active_at: row.get(2)?,
                stage: row.get(3)?,
                checked_at: row.get(4)?,
                messaged_at: row.get(5)?,
                second: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(found)
}

/// Records the activity of a tree, replacing any earlier row.
pub fn set(conn: &Connection, activity: &rows::ClaimActivity) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO claim_activity \
         (tree, ticket, active_at, stage, checked_at, messaged_at, second) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            activity.tree,
            activity.ticket,
            activity.active_at,
            activity.stage,
            activity.checked_at,
            activity.messaged_at,
            activity.second,
        ],
    )?;
    Ok(())
}

/// Forgets a tree whose lease ended.
pub fn clear(conn: &Connection, tree: &str) -> Result<()> {
    conn.execute("DELETE FROM claim_activity WHERE tree = ?1", [tree])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn rows_are_replaced_and_cleared() {
        let conn = open_in_memory().unwrap();
        assert!(all(&conn).unwrap().is_empty());
        let mut row = rows::ClaimActivity {
            tree: "tree-01".to_owned(),
            ticket: "Ta.1".to_owned(),
            active_at: "2027-01-01T00:00:00.000Z".to_owned(),
            stage: "active".to_owned(),
            checked_at: None,
            messaged_at: None,
            second: false,
        };
        set(&conn, &row).unwrap();
        row.stage = "messaged".to_owned();
        row.messaged_at = Some("2027-01-01T00:40:00.000Z".to_owned());
        set(&conn, &row).unwrap();
        assert_eq!(all(&conn).unwrap(), vec![row]);
        clear(&conn, "tree-01").unwrap();
        assert!(all(&conn).unwrap().is_empty());
    }
}
