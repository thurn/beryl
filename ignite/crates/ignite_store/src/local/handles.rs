//! Handles: operations that outlived a client's wait, with their replies once finished.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Result;
use crate::local::rows;

const COLUMNS: &str = "id, verb, session, tree, state, exit, reply, started_at, finished_at";

fn handle(row: &Row) -> rusqlite::Result<rows::Handle> {
    Ok(rows::Handle {
        id: row.get(0)?,
        verb: row.get(1)?,
        session: row.get(2)?,
        tree: row.get(3)?,
        state: row.get(4)?,
        exit: row.get(5)?,
        reply: row.get(6)?,
        started_at: row.get(7)?,
        finished_at: row.get(8)?,
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<rows::Handle>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM handles WHERE id = ?1"),
            [id],
            handle,
        )
        .optional()?)
}

/// Inserts the handle or replaces every column of the existing row, as when it finishes.
pub fn upsert(conn: &Connection, handle: &rows::Handle) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO handles ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        ),
        params![
            handle.id,
            handle.verb,
            handle.session,
            handle.tree,
            handle.state,
            handle.exit,
            handle.reply,
            handle.started_at,
            handle.finished_at,
        ],
    )?;
    Ok(())
}

/// The handles still running, oldest first: what a restarted daemon must find out about.
pub fn running(conn: &Connection) -> Result<Vec<rows::Handle>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM handles WHERE state = 'running' ORDER BY started_at, id"
    ))?;
    let handles = statement
        .query_map([], handle)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(handles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn running_handle(id: &str, at: &str) -> rows::Handle {
        rows::Handle {
            id: id.to_owned(),
            verb: "land".to_owned(),
            session: Some("local_a".to_owned()),
            tree: Some("tree-01".to_owned()),
            state: "running".to_owned(),
            exit: None,
            reply: None,
            started_at: at.to_owned(),
            finished_at: None,
        }
    }

    #[test]
    fn a_finished_handle_keeps_its_reply_and_leaves_the_running_list() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &running_handle("L-2", "t2")).unwrap();
        upsert(&conn, &running_handle("L-1", "t1")).unwrap();
        let ids: Vec<String> = running(&conn).unwrap().into_iter().map(|h| h.id).collect();
        assert_eq!(ids, ["L-1", "L-2"]);

        let mut done = running_handle("L-1", "t1");
        done.state = "done".to_owned();
        done.exit = Some(2);
        done.reply = Some("{\"exit\":2}".to_owned());
        done.finished_at = Some("t3".to_owned());
        upsert(&conn, &done).unwrap();
        assert_eq!(get(&conn, "L-1").unwrap(), Some(done));
        assert_eq!(running(&conn).unwrap().len(), 1);
        assert_eq!(get(&conn, "L-9").unwrap(), None);
    }
}
