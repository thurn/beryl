//! Handles: operations that outlived a client's wait, with their replies once finished.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Result;
use crate::local::rows;

const COLUMNS: &str = "id, verb, session, tree, state, exit, reply, started_at, finished_at, pid, executable, protocol";

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
        pid: row.get(9)?,
        executable: row.get(10)?,
        protocol: row.get(11)?,
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
            "INSERT OR REPLACE INTO handles ({COLUMNS}) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
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
            handle.pid,
            handle.executable,
            handle.protocol,
        ],
    )?;
    Ok(())
}

/// The verb process that runs a handle's verb.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: i64,
    /// The Ignite commit of its executable.
    pub executable: String,
    /// The host protocol version it speaks.
    pub protocol: i64,
}

/// Records the verb process of a running handle. False, and nothing changed, when the handle is
/// unknown or no longer running.
pub fn set_process(conn: &Connection, id: &str, process: &Process) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE handles SET pid = ?2, executable = ?3, protocol = ?4 \
         WHERE id = ?1 AND state = 'running'",
        params![id, process.pid, process.executable, process.protocol],
    )?;
    Ok(changed == 1)
}

/// How a handle's operation ended, as its verb process writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ending<'a> {
    /// `done`, `stopped` or `incomplete`.
    pub state: &'a str,
    pub exit: i64,
    /// JSON: the reply.
    pub reply: &'a str,
    pub finished_at: &'a str,
}

/// Writes how a running handle's operation ended, in one transaction, replacing the request its
/// `reply` held while it ran. False, and nothing changed, when the handle is unknown or no
/// longer running, as when someone else ended it first.
pub fn finish_running(conn: &Connection, id: &str, ending: &Ending) -> Result<bool> {
    let transaction = conn.unchecked_transaction()?;
    let changed = transaction.execute(
        "UPDATE handles SET state = ?2, exit = ?3, reply = ?4, finished_at = ?5 \
         WHERE id = ?1 AND state = 'running'",
        params![
            id,
            ending.state,
            ending.exit,
            ending.reply,
            ending.finished_at
        ],
    )?;
    transaction.commit()?;
    Ok(changed == 1)
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

/// The highest number in any handle's ID, such as 189 for `L-189`, or 0 with no handles. A
/// restarted daemon numbers its next handle above it, so IDs only grow across restarts.
pub fn highest_number(conn: &Connection) -> Result<u64> {
    let highest: i64 = conn.query_row(
        "SELECT COALESCE(MAX(CAST(substr(id, instr(id, '-') + 1) AS INTEGER)), 0) FROM handles",
        [],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(highest).unwrap_or(0))
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
            pid: None,
            executable: None,
            protocol: None,
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

    #[test]
    fn a_verb_process_records_itself_and_ends_its_running_handle_once() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &running_handle("L-1", "t1")).unwrap();
        let process = Process {
            pid: 42,
            executable: "1a2b3c4".to_owned(),
            protocol: 1,
        };
        assert!(set_process(&conn, "L-1", &process).unwrap());
        assert!(!set_process(&conn, "L-9", &process).unwrap());
        let ending = Ending {
            state: "done",
            exit: 0,
            reply: "{}",
            finished_at: "t2",
        };
        assert!(finish_running(&conn, "L-1", &ending).unwrap());
        let row = get(&conn, "L-1").unwrap().unwrap();
        assert_eq!(
            (row.state.as_str(), row.exit, row.reply.as_deref(), row.pid),
            ("done", Some(0), Some("{}"), Some(42))
        );
        assert_eq!(row.executable.as_deref(), Some("1a2b3c4"));
        assert!(!finish_running(&conn, "L-1", &ending).unwrap());
        assert!(!set_process(&conn, "L-1", &process).unwrap());
    }
}
