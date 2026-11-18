//! The pool: the machine's worker and dispatcher sessions.

use rusqlite::{Connection, params};

use crate::Result;
use crate::local::rows;

/// Every member, ordered by session ID.
pub fn all(conn: &Connection) -> Result<Vec<rows::PoolMember>> {
    let mut statement = conn.prepare(
        "SELECT session, app, role, state, ticket, models, added_at FROM pool ORDER BY session",
    )?;
    let members = statement
        .query_map([], |row| {
            Ok(rows::PoolMember {
                session: row.get(0)?,
                app: row.get(1)?,
                role: row.get(2)?,
                state: row.get(3)?,
                ticket: row.get(4)?,
                models: row.get(5)?,
                added_at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(members)
}

/// Inserts the member or replaces every column of the existing row.
pub fn upsert(conn: &Connection, member: &rows::PoolMember) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO pool (session, app, role, state, ticket, models, added_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            member.session,
            member.app,
            member.role,
            member.state,
            member.ticket,
            member.models,
            member.added_at,
        ],
    )?;
    Ok(())
}

/// Removes the session. Removing one that is not in the pool is not an error.
pub fn remove(conn: &Connection, session: &str) -> Result<()> {
    conn.execute("DELETE FROM pool WHERE session = ?1", [session])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn member(session: &str, state: &str) -> rows::PoolMember {
        rows::PoolMember {
            session: session.to_owned(),
            app: "claude".to_owned(),
            role: "worker".to_owned(),
            state: state.to_owned(),
            ticket: None,
            models: "[\"sonnet\"]".to_owned(),
            added_at: "2026-11-18T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn upsert_replaces_the_member_and_remove_deletes_it() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &member("local_b", "idle")).unwrap();
        upsert(&conn, &member("local_a", "idle")).unwrap();
        upsert(&conn, &member("local_b", "working")).unwrap();
        let states: Vec<(String, String)> = all(&conn)
            .unwrap()
            .into_iter()
            .map(|m| (m.session, m.state))
            .collect();
        assert_eq!(
            states,
            [
                ("local_a".to_owned(), "idle".to_owned()),
                ("local_b".to_owned(), "working".to_owned())
            ]
        );
        remove(&conn, "local_a").unwrap();
        remove(&conn, "local_a").unwrap();
        assert_eq!(all(&conn).unwrap().len(), 1);
    }
}
