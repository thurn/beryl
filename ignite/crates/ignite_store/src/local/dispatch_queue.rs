//! The dispatcher's queue of instructions, reservations, refusals and stalls, as JSON rows, so
//! that a restarted daemon gives again what was queued or given and not yet acknowledged.

use rusqlite::{Connection, TransactionBehavior, params};

use crate::Result;
use crate::local::rows;

/// Every entry, in the order it was saved.
pub fn all(conn: &Connection) -> Result<Vec<rows::DispatchEntry>> {
    let mut statement =
        conn.prepare("SELECT kind, key, body FROM dispatch_queue ORDER BY rowid")?;
    let found = statement
        .query_map([], |row| {
            Ok(rows::DispatchEntry {
                kind: row.get(0)?,
                key: row.get(1)?,
                body: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(found)
}

/// Replaces every entry with `entries`, in one transaction, so a crash leaves the old queue or
/// the new one and never half of each.
pub fn replace_all(conn: &mut Connection, entries: &[rows::DispatchEntry]) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute("DELETE FROM dispatch_queue", [])?;
    {
        let mut insert = transaction
            .prepare("INSERT INTO dispatch_queue (kind, key, body) VALUES (?1, ?2, ?3)")?;
        for entry in entries {
            insert.execute(params![entry.kind, entry.key, entry.body])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn entry(kind: &str, key: &str, body: &str) -> rows::DispatchEntry {
        rows::DispatchEntry {
            kind: kind.to_owned(),
            key: key.to_owned(),
            body: body.to_owned(),
        }
    }

    #[test]
    fn the_queue_is_replaced_whole_and_keeps_its_order() {
        let mut conn = open_in_memory().unwrap();
        assert!(all(&conn).unwrap().is_empty());
        let first = vec![entry("pending", "2", "{}"), entry("pending", "1", "{}")];
        replace_all(&mut conn, &first).unwrap();
        assert_eq!(all(&conn).unwrap(), first);
        let second = vec![entry("refused", "local_a", "\"t\"")];
        replace_all(&mut conn, &second).unwrap();
        assert_eq!(all(&conn).unwrap(), second);
    }
}
