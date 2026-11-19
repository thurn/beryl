//! The outbox: records and results not yet sent to the hub, sent in order when it is reachable.

use rusqlite::{Connection, params};

use crate::Result;
use crate::local::rows;

/// Appends an entry and returns its sequence number, which is higher than every earlier
/// entry's, including entries since acknowledged.
pub fn push(conn: &Connection, kind: &str, body: &str, at: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO outbox (kind, body, at) VALUES (?1, ?2, ?3)",
        params![kind, body, at],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The oldest `limit` entries. Peeking removes nothing: an entry stays until [`ack`], so the
/// daemon sends it again after a failure or a restart.
pub fn peek(conn: &Connection, limit: u32) -> Result<Vec<rows::OutboxEntry>> {
    let mut statement =
        conn.prepare("SELECT seq, kind, body, at FROM outbox ORDER BY seq LIMIT ?1")?;
    let entries = statement
        .query_map([limit], |row| {
            Ok(rows::OutboxEntry {
                seq: row.get(0)?,
                kind: row.get(1)?,
                body: row.get(2)?,
                at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(entries)
}

/// Deletes entries up to and including `seq`: those the hub has accepted.
pub fn ack(conn: &Connection, seq: i64) -> Result<()> {
    conn.execute("DELETE FROM outbox WHERE seq <= ?1", [seq])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::{open, open_in_memory};

    #[test]
    fn entries_come_out_in_order_and_stay_until_acknowledged() {
        let conn = open_in_memory().unwrap();
        let first = push(&conn, "record", "{\"n\":1}", "t1").unwrap();
        let second = push(&conn, "result", "{\"n\":2}", "t2").unwrap();
        let third = push(&conn, "flaky", "{\"n\":3}", "t3").unwrap();
        assert!(first < second && second < third);

        let seen: Vec<i64> = peek(&conn, 2).unwrap().iter().map(|e| e.seq).collect();
        assert_eq!(seen, [first, second]);
        // Peeking again gives the same entries: nothing was dropped.
        assert_eq!(peek(&conn, 10).unwrap().len(), 3);

        ack(&conn, second).unwrap();
        let rest = peek(&conn, 10).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!((rest[0].seq, rest[0].kind.as_str()), (third, "flaky"));
    }

    #[test]
    fn a_sequence_number_is_not_reused_after_the_outbox_empties() {
        let conn = open_in_memory().unwrap();
        let first = push(&conn, "record", "{}", "t").unwrap();
        ack(&conn, first).unwrap();
        assert!(peek(&conn, 10).unwrap().is_empty());
        let second = push(&conn, "record", "{}", "t").unwrap();
        assert!(second > first);
        // A late acknowledgement of the first entry leaves the second alone.
        ack(&conn, first).unwrap();
        assert_eq!(peek(&conn, 10).unwrap().len(), 1);
    }

    #[test]
    fn unsent_entries_survive_reopening_the_database() {
        let dir = std::env::temp_dir().join(format!("ignite-outbox-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("local.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = open(&path).unwrap();
            push(&conn, "record", "{\"a\":1}", "t1").unwrap();
            push(&conn, "result", "{\"b\":2}", "t2").unwrap();
        }
        let conn = open(&path).unwrap();
        let bodies: Vec<String> = peek(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|e| e.body)
            .collect();
        assert_eq!(bodies, ["{\"a\":1}", "{\"b\":2}"]);
        // Windows deletes no file that a connection still holds open.
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
