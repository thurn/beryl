//! The `ticket_spend` table: each ticket window's time and dollar segments, which the
//! dashboard's ticket pages draw in their efficiency section.
//!
//! A window is one JSON object as the efficiency dashboard's `spend` command writes it. The
//! hub reads only `ticket`, `window`, `start` and `end` from it; the segments are kept as they
//! came and are read by the dashboard.

use ignite_core::time::Timestamp;
use rusqlite::{Connection, params, params_from_iter};
use serde_json::Value;

use crate::Result;

/// The source name under which machines post spend windows to `POST /metrics/import`.
pub const SOURCE: &str = "spend";

/// One stored window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub machine: String,
    pub ticket: String,
    pub start: String,
    pub end: String,
    /// The window as posted, as JSON.
    pub data: String,
}

/// Saves `window`, posted by `machine`, replacing an earlier post of the same window. Returns
/// whether it was stored: a window that names no ticket, key or times is skipped.
pub fn record(conn: &Connection, machine: &str, window: &Value, now: Timestamp) -> Result<bool> {
    let text = |key: &str| window.get(key).and_then(Value::as_str).unwrap_or("");
    let (ticket, key, start, end) = (text("ticket"), text("window"), text("start"), text("end"));
    if ticket.is_empty() || key.is_empty() || start.is_empty() || end.is_empty() {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO ticket_spend (machine, window, ticket, start, end, data, at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(machine, window) DO UPDATE SET ticket = excluded.ticket, \
         start = excluded.start, end = excluded.end, data = excluded.data, at = excluded.at",
        params![
            machine,
            key,
            ticket,
            start,
            end,
            window.to_string(),
            now.to_string()
        ],
    )?;
    Ok(true)
}

/// Every window of the tickets `ids`, oldest first.
pub fn for_tickets(conn: &Connection, ids: &[String]) -> Result<Vec<Window>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    // SQLite's limit on a statement's parameters is far above any epic's size, but a chunk
    // keeps one statement's text small.
    for chunk in ids.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(", ");
        let mut statement = conn.prepare(&format!(
            "SELECT machine, ticket, start, end, data FROM ticket_spend \
             WHERE ticket IN ({marks})"
        ))?;
        let rows = statement.query_map(params_from_iter(chunk), |row| {
            Ok(Window {
                machine: row.get(0)?,
                ticket: row.get(1)?,
                start: row.get(2)?,
                end: row.get(3)?,
                data: row.get(4)?,
            })
        })?;
        for row in rows {
            found.push(row?);
        }
    }
    found.sort_by(|left, right| left.start.cmp(&right.start));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_window_posted_again_replaces_its_row() {
        let conn = crate::hub::open_in_memory().unwrap();
        let now = Timestamp::parse("2026-11-19T06:00:00Z").unwrap();
        let window = |end: &str| {
            json!({"ticket": "TA", "window": "claude:t1:s", "start": "2026-11-19T05:00:00Z",
                   "end": end, "time": [], "dollars": []})
        };
        assert!(record(&conn, "m-1", &window("2026-11-19T05:10:00Z"), now).unwrap());
        assert!(record(&conn, "m-1", &window("2026-11-19T05:20:00Z"), now).unwrap());
        assert!(record(&conn, "m-2", &window("2026-11-19T05:20:00Z"), now).unwrap());
        assert!(!record(&conn, "m-1", &json!({"ticket": "TA"}), now).unwrap());
        let found = for_tickets(&conn, &["TA".to_owned(), "TB".to_owned()]).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].end, "2026-11-19T05:20:00Z");
        assert!(for_tickets(&conn, &["TB".to_owned()]).unwrap().is_empty());
    }
}
