//! What each leased tree's session has been shown of its ticket's comments and history, so that
//! a change made to the ticket is printed once and a restarted daemon does not print it again.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::local::rows;

/// The marks recorded for `tree`, if any. A row written for another ticket is returned as it
/// is; the caller compares its `ticket`.
pub fn get(conn: &Connection, tree: &str) -> Result<Option<rows::Shown>> {
    Ok(conn
        .query_row(
            "SELECT tree, ticket, comment, history FROM shown WHERE tree = ?1",
            [tree],
            |row| {
                Ok(rows::Shown {
                    tree: row.get(0)?,
                    ticket: row.get(1)?,
                    comment: row.get(2)?,
                    history: row.get(3)?,
                })
            },
        )
        .optional()?)
}

/// Records the marks for a tree, replacing any earlier row.
pub fn set(conn: &Connection, shown: &rows::Shown) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO shown (tree, ticket, comment, history) VALUES (?1, ?2, ?3, ?4)",
        params![shown.tree, shown.ticket, shown.comment, shown.history],
    )?;
    Ok(())
}

/// Forgets the marks for a tree whose lease ended.
pub fn clear(conn: &Connection, tree: &str) -> Result<()> {
    conn.execute("DELETE FROM shown WHERE tree = ?1", [tree])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn marks_are_replaced_and_cleared() {
        let conn = open_in_memory().unwrap();
        assert_eq!(get(&conn, "tree-01").unwrap(), None);
        let mut shown = rows::Shown {
            tree: "tree-01".to_owned(),
            ticket: "Ta.1".to_owned(),
            comment: 2,
            history: 7,
        };
        set(&conn, &shown).unwrap();
        shown.comment = 3;
        set(&conn, &shown).unwrap();
        assert_eq!(get(&conn, "tree-01").unwrap(), Some(shown));
        clear(&conn, "tree-01").unwrap();
        assert_eq!(get(&conn, "tree-01").unwrap(), None);
    }
}
