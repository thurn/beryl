//! Replays a read-only hub database snapshot at upkeep's normal one-minute cadence.
//! Set `IGNITE_REGRESSION_REPLAY_DB` to the snapshot and run with `--ignored --nocapture`.

use std::collections::BTreeMap;

use ignite_core::time::Timestamp;
use rusqlite::{Connection, OpenFlags};

use super::{PASS_SECONDS, clearing, filing, justiciar, regression_stalls};

#[derive(Default)]
struct Escalation {
    first: Option<Timestamp>,
    cleared_since: Option<Timestamp>,
    closed_at: Option<Timestamp>,
    returned: bool,
}

impl Escalation {
    fn observe(&mut self, now: Timestamp, stalled: bool) -> Option<&'static str> {
        if stalled {
            self.cleared_since = None;
            if self.first.is_none() {
                self.first = Some(now);
                return Some("file");
            }
            if let Some(closed) = self.closed_at.take() {
                self.returned = now.seconds_since(closed) <= filing::REOPEN_SECONDS;
                return Some(if self.returned { "reopen" } else { "file" });
            }
        } else if self.first.is_some() && self.closed_at.is_none() {
            let cleared = self.cleared_since.get_or_insert(now);
            let hold = if self.returned {
                clearing::RETURNED_HOLD_SECONDS
            } else {
                clearing::HOLD_SECONDS
            };
            if now.seconds_since(*cleared) >= hold {
                self.closed_at = Some(now);
            }
        }
        None
    }
}

#[test]
#[ignore = "requires a read-only hub database snapshot"]
fn replay_regression_stalls() {
    let path = std::env::var_os("IGNITE_REGRESSION_REPLAY_DB").expect("path to a hub DB snapshot");
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    history_views(&conn);
    let (first, last): (String, String) = conn
        .query_row(
            "SELECT (SELECT min(created_at) FROM main.tickets WHERE id IN replay_ids),
                    (SELECT max(at) FROM main.history)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut now = Timestamp::parse(&first).unwrap();
    let last = Timestamp::parse(&last).unwrap();
    let mut escalations = BTreeMap::<String, Escalation>::new();
    while now <= last {
        conn.execute("UPDATE replay_clock SET cutoff = ?1", [now.to_string()])
            .unwrap();
        let stalled: BTreeMap<_, _> = if justiciar::master_lagging(&conn, now).unwrap() {
            // The historical export records overall tip verification. Use that era's
            // passing-tip inference when reconstructing the fault at each minute.
            regression_stalls::open_with(&conn, now, |_, _| Ok(true))
                .unwrap()
                .into_iter()
                .filter_map(|(ticket, stall)| stall.map(|stall| (ticket, stall)))
                .collect()
        } else {
            BTreeMap::new()
        };
        for (ticket, reason) in &stalled {
            let escalation = escalations.entry(ticket.clone()).or_default();
            if let Some(action) = escalation.observe(now, true) {
                println!("{now} {action} {ticket} {}", reason.reason());
            }
        }
        for (ticket, escalation) in &mut escalations {
            if !stalled.contains_key(ticket) {
                escalation.observe(now, false);
            }
        }
        now = now.plus_seconds(f64::from(u32::try_from(PASS_SECONDS).unwrap()));
    }
    let total: u32 = conn
        .query_row("SELECT count(*) FROM replay_ids", [], |row| row.get(0))
        .unwrap();
    println!(
        "{} of {total} regressions would escalate",
        escalations.len()
    );
}

/// Shadows the live tables with the facts that were visible at the replay clock. The source
/// keeps current check ownership but does not keep all of its history. Tickets whose checks
/// later moved still take part in the replay of claims, inactivity and lifetime. Old snapshots
/// without failed-tip evidence cannot attribute a failed tip to a check or a fix.
fn history_views(conn: &Connection) {
    conn.execute_batch(
        "CREATE TEMP TABLE replay_clock (cutoff TEXT NOT NULL);
         INSERT INTO replay_clock VALUES ('');
         CREATE TEMP VIEW history AS
             SELECT h.* FROM main.history h, replay_clock WHERE h.at <= cutoff;
         CREATE TEMP TABLE replay_ids (id TEXT PRIMARY KEY) WITHOUT ROWID;
         INSERT INTO replay_ids
             SELECT id FROM main.tickets t
             WHERE EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = 'regression')
             UNION SELECT ticket FROM main.regressions;
         CREATE TEMP TABLE replay_landings (ticket TEXT, at TEXT, landed_at TEXT);
         INSERT INTO replay_landings
             SELECT ticket, at, json_extract(change, '$.landed_at[1]') FROM main.history
             WHERE json_type(change, '$.landed_at') IS NOT NULL;
         INSERT INTO replay_landings
             SELECT id, landed_at, landed_at FROM main.tickets
             WHERE landed_at IS NOT NULL AND id NOT IN (SELECT ticket FROM replay_landings);
         CREATE INDEX replay_landings_by_time ON replay_landings (at);
         CREATE INDEX replay_landings_by_ticket ON replay_landings (ticket, at);",
    )
    .unwrap();
    let optional_column = |table: &str, column: &str| {
        let present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
                [table, column],
                |row| row.get(0),
            )
            .unwrap();
        if present {
            column.to_owned()
        } else {
            format!("NULL AS {column}")
        }
    };
    let failure = optional_column("tips", "failure");
    let platform = optional_column("regressions", "platform");
    let historical_columns = [
        "status",
        "session",
        "assignee",
        "claimed_at",
        "closed_at",
        "metadata",
    ]
    .map(|field| {
        format!(
            "(SELECT json_extract(change, '$.{field}[1]') FROM history
                  WHERE ticket = t.id AND json_type(change, '$.{field}') IS NOT NULL
                  ORDER BY seq DESC LIMIT 1) AS {field}"
        )
    })
    .join(", ");
    conn.execute_batch(&format!(
        "CREATE TEMP VIEW regressions AS
             SELECT ticket, check_name, 'open' AS state, {platform} FROM main.regressions
             UNION ALL SELECT id, 'historical:' || id, 'open', NULL FROM replay_ids
             WHERE id NOT IN (SELECT ticket FROM main.regressions);
         CREATE TEMP VIEW tips AS
             SELECT commit_sha, seen, verified, state, {failure}
             FROM main.tips, replay_clock WHERE verified <= cutoff;
         CREATE TEMP VIEW tickets AS
             SELECT id, created_at, labels, {historical_columns},
                    (SELECT l.landed_at FROM replay_landings l
                     WHERE l.ticket = t.id AND l.at <= cutoff
                     ORDER BY l.at DESC LIMIT 1) AS landed_at
             FROM replay_ids JOIN main.tickets t USING (id), replay_clock
             WHERE created_at <= cutoff
             UNION ALL
             SELECT ticket, '', '[]', NULL, NULL, NULL, NULL, NULL, NULL, landed_at
             FROM (
                 SELECT ticket, landed_at,
                        row_number() OVER (PARTITION BY ticket ORDER BY at DESC) AS newest
                 FROM replay_landings, replay_clock
                 WHERE at <= cutoff AND ticket NOT IN replay_ids
                   AND at > strftime('%Y-%m-%dT%H:%M:%fZ', cutoff, '-7 days')
             ) WHERE newest = 1;"
    ))
    .unwrap();
}
