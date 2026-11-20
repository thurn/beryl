//! The `transcript_lines` table: the digest of each CLI worker's transcript, which the dashboard
//! shows at `/pool/session/<id>` and `ignite pool watch` follows.

use rusqlite::{Connection, params};

use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::cli::{
    DIGEST_LINE_CHARS, StoredTranscriptLine, TRANSCRIPT_KEEP_DAYS, TranscriptKind, TranscriptLine,
};
use ignite_core::time::Timestamp;

use crate::Result;

/// The most lines one read returns.
pub const PAGE_LINES: usize = 1000;

fn kind_name(kind: TranscriptKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn kind_from(name: &str) -> Option<TranscriptKind> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

/// Stores `lines` that `machine` sent at `now` and returns how many were new. A line whose
/// session, turn and index are already stored is left as it is, so sending a batch again
/// changes nothing. Text longer than [`DIGEST_LINE_CHARS`] is cut.
pub fn append(
    conn: &Connection,
    machine: &MachineId,
    lines: &[TranscriptLine],
    now: Timestamp,
) -> Result<u32> {
    let tx = conn.unchecked_transaction()?;
    let mut stored = 0;
    for line in lines {
        let text: String = line.text.chars().take(DIGEST_LINE_CHARS).collect();
        stored += tx.execute(
            "INSERT OR IGNORE INTO transcript_lines
               (machine, session, ticket, turn, line_index, kind, text, at, stored_at,
                call_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                machine.as_str(),
                line.session.as_str(),
                line.ticket.as_ref().map(TicketId::as_str),
                line.turn,
                line.index,
                kind_name(line.kind),
                text,
                line.at.map(|at| at.to_string()),
                now.to_string(),
                line.call,
            ],
        )? as u32;
    }
    tx.commit()?;
    Ok(stored)
}

type Raw = (
    i64,
    String,
    String,
    Option<String>,
    u32,
    u32,
    String,
    String,
    Option<String>,
    Option<u32>,
);

const COLUMNS: &str =
    "seq, machine, session, ticket, turn, line_index, kind, text, at, call_index";

fn decode(raw: Raw) -> Option<StoredTranscriptLine> {
    let (seq, machine, session, ticket, turn, index, kind, text, at, call) = raw;
    Some(StoredTranscriptLine {
        seq,
        machine: MachineId::new(machine).ok()?,
        line: TranscriptLine {
            session: SessionId::new(session).ok()?,
            ticket: ticket.and_then(|ticket| TicketId::new(ticket).ok()),
            turn,
            index,
            kind: kind_from(&kind)?,
            text,
            at: at.and_then(|at| Timestamp::parse(&at).ok()),
            call,
        },
    })
}

fn raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<Raw> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

/// The lines of `session` stored after `seq`, oldest first, at most [`PAGE_LINES`].
pub fn after(conn: &Connection, session: &str, seq: i64) -> Result<Vec<StoredTranscriptLine>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM transcript_lines WHERE session = ?1 AND seq > ?2
         ORDER BY seq LIMIT ?3"
    ))?;
    let rows = statement.query_map(params![session, seq, PAGE_LINES as i64], raw)?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter_map(decode)
        .collect())
}

/// The last `count` lines of `session`, oldest first.
pub fn tail(conn: &Connection, session: &str, count: usize) -> Result<Vec<StoredTranscriptLine>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM (
           SELECT {COLUMNS} FROM transcript_lines WHERE session = ?1 ORDER BY seq DESC LIMIT ?2)
         ORDER BY seq"
    ))?;
    let rows = statement.query_map(params![session, count as i64], raw)?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter_map(decode)
        .collect())
}

/// Deletes lines that arrived more than [`TRANSCRIPT_KEEP_DAYS`] days before `now` and returns
/// how many.
pub fn delete_expired(conn: &Connection, now: Timestamp) -> Result<usize> {
    let cutoff = now
        .plus_seconds(-TRANSCRIPT_KEEP_DAYS * 86_400.0)
        .to_string();
    Ok(conn.execute(
        "DELETE FROM transcript_lines WHERE stored_at < ?1",
        [cutoff],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(turn: u32, index: u32, text: &str) -> TranscriptLine {
        TranscriptLine {
            session: SessionId::new("cli-claude-0a1b2c3d").unwrap(),
            ticket: Some(TicketId::new("T.1").unwrap()),
            turn,
            index,
            kind: TranscriptKind::Tool,
            text: text.to_owned(),
            at: None,
            call: None,
        }
    }

    #[test]
    fn the_same_line_appended_twice_is_stored_once() {
        let conn = crate::hub::open_in_memory().unwrap();
        let machine = MachineId::new("m-1").unwrap();
        let now = Timestamp::parse("2026-11-20T06:00:00Z").unwrap();
        let lines = [line(1, 0, "Bash: ignite status"), line(1, 1, "Read: a.rs")];
        assert_eq!(append(&conn, &machine, &lines, now).unwrap(), 2);
        assert_eq!(append(&conn, &machine, &lines, now).unwrap(), 0);
        let stored = after(&conn, "cli-claude-0a1b2c3d", 0).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].line.text, "Bash: ignite status");
        assert_eq!(
            after(&conn, "cli-claude-0a1b2c3d", stored[0].seq)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            tail(&conn, "cli-claude-0a1b2c3d", 1).unwrap()[0].line.index,
            1
        );
    }

    #[test]
    fn a_tool_result_keeps_the_index_of_its_call() {
        let conn = crate::hub::open_in_memory().unwrap();
        let machine = MachineId::new("m-1").unwrap();
        let now = Timestamp::parse("2026-11-20T06:00:00Z").unwrap();
        let result = TranscriptLine {
            kind: TranscriptKind::ToolResult,
            call: Some(0),
            ..line(1, 1, "a.rs")
        };
        append(&conn, &machine, &[line(1, 0, "Bash: ls"), result], now).unwrap();
        let stored = after(&conn, "cli-claude-0a1b2c3d", 0).unwrap();
        assert_eq!(
            stored.iter().map(|s| (s.line.kind, s.line.call)).collect::<Vec<_>>(),
            [(TranscriptKind::Tool, None), (TranscriptKind::ToolResult, Some(0))]
        );
    }

    #[test]
    fn lines_older_than_the_keep_period_are_deleted() {
        let conn = crate::hub::open_in_memory().unwrap();
        let machine = MachineId::new("m-1").unwrap();
        let old = Timestamp::parse("2026-11-10T06:00:00Z").unwrap();
        let new = Timestamp::parse("2026-11-19T06:00:00Z").unwrap();
        append(&conn, &machine, &[line(1, 0, "old")], old).unwrap();
        append(&conn, &machine, &[line(2, 0, "new")], new).unwrap();
        let now = Timestamp::parse("2026-11-20T06:00:00Z").unwrap();
        assert_eq!(delete_expired(&conn, now).unwrap(), 1);
        let left = after(&conn, "cli-claude-0a1b2c3d", 0).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].line.text, "new");
    }
}
