//! The `attachments` table: the images attached to each ticket (see
//! `ignite_core::protocol::attachments`). The files live on the hub's disk under their SHA-256;
//! this module only keeps the rows, and answers how much the kept files take up, which hub
//! upkeep compares with the budget.
//!
//! An upload and a removal each take a feed position and record a change of the ticket, so the
//! dashboard and every machine's ticket mirror see it. An upload also adds an entry to the
//! ticket's changelog.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;

use ignite_core::ids::TicketId;
use ignite_core::protocol::attachments::{Attachment, ImageInfo, numbered_name};
use ignite_core::protocol::tickets::FeedChange;
use ignite_core::time::Timestamp;

use super::feed;
use crate::{Result, StoreError};

/// The key of a changelog entry that records an upload: `{"attachment": [null, <name>]}`.
pub const HISTORY_KEY: &str = "attachment";

/// An upload the hub has already written to disk.
#[derive(Clone, Copy, Debug)]
pub struct Upload<'a> {
    pub ticket: &'a TicketId,
    /// The name asked for, already cleaned; a taken name gets `-2`, `-3`.
    pub name: &'a str,
    pub sha256: &'a str,
    pub image: ImageInfo,
    pub bytes: u64,
    pub caption: Option<&'a str>,
}

const COLUMNS: &str = "ticket, name, sha256, media_type, bytes, width, height, caption, \
                       created_at, created_by, removed_at";

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<Attachment>> {
    let ticket: String = row.get(0)?;
    let created_at: String = row.get(8)?;
    let removed_at: Option<String> = row.get(10)?;
    let (Ok(ticket), Ok(created_at)) = (TicketId::new(ticket), Timestamp::parse(&created_at))
    else {
        return Ok(None);
    };
    Ok(Some(Attachment {
        ticket,
        name: row.get(1)?,
        sha256: row.get(2)?,
        media_type: row.get(3)?,
        bytes: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
        width: u32::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
        height: u32::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
        caption: row.get(7)?,
        created_at,
        created_by: row.get(9)?,
        removed_at: removed_at.and_then(|at| Timestamp::parse(&at).ok()),
    }))
}

/// Records `upload` by `actor`. When the ticket already keeps the same file under the name asked
/// for, or under one of its numbered forms, that row is returned unchanged, so a client that
/// sends an upload again after an unclear failure does not attach the image twice.
pub fn insert(
    conn: &mut Connection,
    upload: &Upload<'_>,
    actor: &str,
    now: Timestamp,
) -> Result<Attachment> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut n = 1;
    let name = loop {
        let candidate = numbered_name(upload.name, n);
        match get(&tx, upload.ticket.as_str(), &candidate)? {
            None => break candidate,
            Some(found) if found.sha256 == upload.sha256 && found.removed_at.is_none() => {
                return Ok(found);
            }
            Some(_) => n += 1,
        }
    };
    tx.execute(
        &format!("INSERT INTO attachments ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)"),
        params![
            upload.ticket.as_str(),
            name,
            upload.sha256,
            upload.image.media_type,
            i64::try_from(upload.bytes).unwrap_or(i64::MAX),
            upload.image.width,
            upload.image.height,
            upload.caption,
            now.to_string(),
            actor,
        ],
    )?;
    let seq = feed::next_seq(&tx)?;
    tx.execute(
        "INSERT INTO history (ticket, seq, actor, at, change) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            upload.ticket.as_str(),
            seq.0 as i64,
            actor,
            now.to_string(),
            json!({ HISTORY_KEY: [null, name] }).to_string(),
        ],
    )?;
    record_change(&tx, upload.ticket, seq)?;
    let stored = get(&tx, upload.ticket.as_str(), &name)?
        .ok_or_else(|| StoreError::Invalid(format!("attachment {name} was not stored")))?;
    tx.commit()?;
    Ok(stored)
}

fn record_change(conn: &Connection, ticket: &TicketId, seq: ignite_core::ids::Seq) -> Result<()> {
    feed::record(
        conn,
        &FeedChange {
            seq,
            kind: "ticket".to_owned(),
            ticket: Some(ticket.clone()),
            name: None,
        },
    )
}

/// The attachment `name` of `ticket`.
pub fn get(conn: &Connection, ticket: &str, name: &str) -> Result<Option<Attachment>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM attachments WHERE ticket = ?1 AND name = ?2"),
            params![ticket, name],
            decode,
        )
        .optional()?
        .flatten())
}

/// Every attachment of `ticket`, oldest first.
pub fn list(conn: &Connection, ticket: &str) -> Result<Vec<Attachment>> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM attachments WHERE ticket = ?1 ORDER BY created_at, name"
    ))?;
    let rows = statement.query_map([ticket], decode)?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// The media type of the file `sha256` when some attachment still keeps it, so the hub may
/// serve it; `None` when no row names it or every row that does was removed.
pub fn kept_media_type(conn: &Connection, sha256: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT media_type FROM attachments WHERE sha256 = ?1 AND removed_at IS NULL LIMIT 1",
            [sha256],
            |row| row.get(0),
        )
        .optional()?)
}

/// Every file some attachment still keeps.
pub fn kept_files(conn: &Connection) -> Result<std::collections::BTreeSet<String>> {
    let mut statement =
        conn.prepare("SELECT DISTINCT sha256 FROM attachments WHERE removed_at IS NULL")?;
    let rows = statement.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The total size of the files kept by attachments, each file counted once. With `open_only`,
/// only files kept by a ticket that is not closed.
pub fn kept_bytes(conn: &Connection, open_only: bool) -> Result<u64> {
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(bytes), 0) FROM (
           SELECT a.sha256, MAX(a.bytes) AS bytes FROM attachments a
           JOIN tickets t ON t.id = a.ticket
           WHERE a.removed_at IS NULL AND (?1 = 0 OR t.status != 'closed')
           GROUP BY a.sha256)",
        [i64::from(open_only)],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(total).unwrap_or(0))
}

/// The closed tickets that still keep an attachment, with when each closed, the ticket closed
/// longest ago first.
pub fn closed_with_files(conn: &Connection) -> Result<Vec<(TicketId, Timestamp)>> {
    let mut statement = conn.prepare(
        "SELECT t.id, t.closed_at FROM tickets t
         WHERE t.status = 'closed' AND t.closed_at IS NOT NULL
           AND EXISTS (SELECT 1 FROM attachments a WHERE a.ticket = t.id AND a.removed_at IS NULL)
         ORDER BY t.closed_at, t.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut found = Vec::new();
    for row in rows {
        let (id, closed_at) = row?;
        if let (Ok(id), Ok(at)) = (TicketId::new(id), Timestamp::parse(&closed_at)) {
            found.push((id, at));
        }
    }
    Ok(found)
}

/// Marks every kept attachment of `ticket` removed at `now` and returns how many. The files
/// themselves are deleted by the caller once no kept row names them.
pub fn remove_all(conn: &mut Connection, ticket: &TicketId, now: Timestamp) -> Result<usize> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let removed = tx.execute(
        "UPDATE attachments SET removed_at = ?2 WHERE ticket = ?1 AND removed_at IS NULL",
        params![ticket.as_str(), now.to_string()],
    )?;
    if removed > 0 {
        let seq = feed::next_seq(&tx)?;
        record_change(&tx, ticket, seq)?;
    }
    tx.commit()?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::protocol::attachments::sniff;

    fn ticket(conn: &Connection, id: &str, status: &str, closed_at: Option<&str>) -> TicketId {
        conn.execute(
            "INSERT INTO tickets (id, title, type, status, priority, labels, metadata, created_at,
               created_by, updated_at, closed_at, version)
             VALUES (?1, 't', 'task', ?2, 2, '[]', '{}', '2026-01-01T00:00:00Z', 'x',
               '2026-01-01T00:00:00Z', ?3, 1)",
            params![id, status, closed_at],
        )
        .unwrap();
        TicketId::new(id).unwrap()
    }

    fn upload<'a>(ticket: &'a TicketId, name: &'a str, sha: &'a str, bytes: u64) -> Upload<'a> {
        let header = b"GIF89a\x02\x00\x03\x00";
        Upload {
            ticket,
            name,
            sha256: sha,
            image: sniff(header).unwrap(),
            bytes,
            caption: None,
        }
    }

    fn now() -> Timestamp {
        Timestamp::parse("2026-11-20T06:00:00Z").unwrap()
    }

    #[test]
    fn a_taken_name_is_numbered_and_the_same_upload_again_is_kept_once() {
        let mut conn = crate::hub::open_in_memory().unwrap();
        let id = ticket(&conn, "T.1", "open", None);
        let first = insert(&mut conn, &upload(&id, "shot.gif", "aa", 10), "me", now()).unwrap();
        let second = insert(&mut conn, &upload(&id, "shot.gif", "bb", 20), "me", now()).unwrap();
        assert_eq!(
            (first.name.as_str(), second.name.as_str()),
            ("shot.gif", "shot-2.gif")
        );
        // The second upload sent again finds itself under its numbered name.
        let again = insert(&mut conn, &upload(&id, "shot.gif", "bb", 20), "me", now()).unwrap();
        assert_eq!(again, second);
        assert_eq!(list(&conn, "T.1").unwrap().len(), 2);
        assert_eq!((first.width, first.height), (2, 3));
        // Each upload took one feed position and left one changelog entry.
        assert_eq!(feed::current(&conn).unwrap().0, 2);
        let entries: i64 = conn
            .query_row(
                "SELECT count(*) FROM history WHERE ticket = 'T.1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(entries, 2);
    }

    #[test]
    fn kept_bytes_count_each_file_once_and_open_tickets_apart() {
        let mut conn = crate::hub::open_in_memory().unwrap();
        let open = ticket(&conn, "T.1", "open", None);
        let closed = ticket(&conn, "T.2", "closed", Some("2026-02-01T00:00:00Z"));
        insert(&mut conn, &upload(&open, "a.gif", "aa", 10), "me", now()).unwrap();
        insert(&mut conn, &upload(&closed, "a.gif", "aa", 10), "me", now()).unwrap();
        insert(&mut conn, &upload(&closed, "b.gif", "bb", 30), "me", now()).unwrap();
        assert_eq!(kept_bytes(&conn, false).unwrap(), 40);
        assert_eq!(kept_bytes(&conn, true).unwrap(), 10);
        assert_eq!(closed_with_files(&conn).unwrap().len(), 1);
        assert_eq!(remove_all(&mut conn, &closed, now()).unwrap(), 2);
        assert_eq!(kept_bytes(&conn, false).unwrap(), 10);
        assert_eq!(
            kept_media_type(&conn, "aa").unwrap().as_deref(),
            Some("image/gif")
        );
        assert_eq!(kept_media_type(&conn, "bb").unwrap(), None);
        let removed = get(&conn, "T.2", "b.gif").unwrap().unwrap();
        assert_eq!(removed.removed_at, Some(now()));
    }
}
