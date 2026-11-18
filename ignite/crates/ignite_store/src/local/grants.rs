//! Grants of cores and resources to running commands, so a restarted daemon knows what is held.

use rusqlite::{Connection, params};

use crate::Result;
use crate::local::rows;

/// Every grant not yet removed, oldest first. The `id` of the grant passed to [`insert`] is
/// ignored and the stored row's is returned.
pub fn active(conn: &Connection) -> Result<Vec<rows::Grant>> {
    let mut statement = conn.prepare(
        "SELECT id, handle, class, cores, resources, tree, ticket, pid, granted_at \
         FROM grants ORDER BY id",
    )?;
    let grants = statement
        .query_map([], |row| {
            Ok(rows::Grant {
                id: row.get(0)?,
                handle: row.get(1)?,
                class: row.get(2)?,
                cores: row.get(3)?,
                resources: row.get(4)?,
                tree: row.get(5)?,
                ticket: row.get(6)?,
                pid: row.get(7)?,
                granted_at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(grants)
}

/// Stores a new grant and returns its ID.
pub fn insert(conn: &Connection, grant: &rows::Grant) -> Result<i64> {
    conn.execute(
        "INSERT INTO grants (handle, class, cores, resources, tree, ticket, pid, granted_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            grant.handle,
            grant.class,
            grant.cores,
            grant.resources,
            grant.tree,
            grant.ticket,
            grant.pid,
            grant.granted_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Releases a grant. Removing one that is already gone is not an error.
pub fn remove(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM grants WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn grant(handle: &str, cores: i64) -> rows::Grant {
        rows::Grant {
            id: 0,
            handle: Some(handle.to_owned()),
            class: "agent".to_owned(),
            cores,
            resources: "[\"browser\"]".to_owned(),
            tree: Some("tree-01".to_owned()),
            ticket: None,
            pid: Some(4242),
            granted_at: "2026-11-18T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn a_grant_is_held_until_removed() {
        let conn = open_in_memory().unwrap();
        let a = insert(&conn, &grant("L-1", 4)).unwrap();
        let b = insert(&conn, &grant("L-2", 2)).unwrap();
        assert_ne!(a, b);
        let held = active(&conn).unwrap();
        assert_eq!(
            held.iter().map(|g| (g.id, g.cores)).collect::<Vec<_>>(),
            [(a, 4), (b, 2)]
        );
        assert_eq!(held[0].resources, "[\"browser\"]");

        remove(&conn, a).unwrap();
        remove(&conn, a).unwrap();
        assert_eq!(
            active(&conn)
                .unwrap()
                .iter()
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            [b]
        );
    }
}
