//! Every ticket, with its aliases, edges, comments and history, from the old ticket service's
//! database file, which is only read.
//!
//! The hub's ticket tables have the old service's columns under the same names (`tickets` adds
//! columns of its own, which an import leaves alone), so each table is copied column for column.
//! A row is written only when the hub has no row with its key, so a second import writes
//! nothing and an import run after the hub went live never reverts newer hub data:
//!
//! - A ticket the hub already holds is overwritten only when the imported row's `updated_at` is
//!   later than the hub's.
//! - An edge, alias or history row the hub already holds is left as it is.
//! - A comment whose `(ticket, n)` the hub holds with another text is appended to the ticket as
//!   its next comment (unless the ticket already has that text, as from an earlier import), and
//!   the report says so.
//!
//! Two kinds of old data are left out, because Ignite would misread them:
//!
//! - An edge from a ticket to itself, which would show the ticket as waiting on itself. It is
//!   counted as skipped and named in the report.
//! - A `role:*` label, which named the old service's kind of worker for the ticket. Ignite hands
//!   tickets to any worker, and a role label on a ticket in its ready list reads as an
//!   instruction nobody follows.
//!
//! The old service's `events` and `leases` are not imported: events were its own log of sessions
//! and CI, and leases are locks held by processes that stop at the switch. The global change counter `seq` moves up to the old service's, so
//! changes after the import number above every imported ticket's `version`.

use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params_from_iter};

use ignite_core::ids::Seq;

use crate::Result;
use crate::hub::feed;
use crate::import::{ImportReport, open_source};

/// A table copied from the old database: its name, its key columns and its other columns.
struct Table {
    name: &'static str,
    kind: &'static str,
    key: &'static [&'static str],
    columns: &'static [&'static str],
    /// What to do with a row whose key the hub already holds.
    existing: Existing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Existing {
    /// Overwrite when the imported row's `updated_at` is later.
    IfNewer,
    /// Leave the hub's row.
    Keep,
    /// A comment: append the imported one if the text differs.
    AppendComment,
}

const TABLES: &[Table] = &[
    Table {
        name: "tickets",
        kind: "tickets",
        key: &["id"],
        columns: &[
            "parent",
            "next_child",
            "title",
            "body",
            "acceptance",
            "design",
            "notes",
            "type",
            "status",
            "priority",
            "assignee",
            "last",
            "labels",
            "metadata",
            "defer_until",
            "close_reason",
            "created_at",
            "created_by",
            "updated_at",
            "closed_at",
            "version",
        ],
        existing: Existing::IfNewer,
    },
    Table {
        name: "edges",
        kind: "edges",
        key: &["src", "dst"],
        columns: &["kind"],
        existing: Existing::Keep,
    },
    Table {
        name: "comments",
        kind: "comments",
        key: &["ticket", "n"],
        columns: &["author", "at", "text", "legacy_id"],
        existing: Existing::AppendComment,
    },
    Table {
        name: "history",
        kind: "history",
        key: &["ticket", "seq"],
        columns: &["actor", "at", "change"],
        existing: Existing::Keep,
    },
    Table {
        name: "aliases",
        kind: "aliases",
        key: &["alias"],
        columns: &["id"],
        existing: Existing::Keep,
    },
];

/// The old tables that are read only to be counted as skipped.
const SKIPPED: &[&str] = &["events", "leases"];

pub fn import(hub: &Connection, old_database: &Path) -> Result<ImportReport> {
    let old = open_source(old_database)?;
    let mut report = ImportReport::new(&format!("tickets ({})", old_database.display()));
    for table in TABLES {
        copy(&old, hub, table, &mut report)?;
    }
    for table in SKIPPED {
        let rows: i64 = old.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })?;
        let count = report.kind(table);
        count.read += rows as u64;
        count.skipped += rows as u64;
    }
    raise_seq(&old, hub, &mut report)?;
    Ok(report)
}

/// Copies one table: an insert, or an update of a row whose values differ.
fn copy(
    old: &Connection,
    hub: &Connection,
    table: &Table,
    report: &mut ImportReport,
) -> Result<()> {
    let all: Vec<&str> = table.key.iter().chain(table.columns).copied().collect();
    let quoted: Vec<String> = all.iter().map(|c| format!("\"{c}\"")).collect();
    let mut select = old.prepare(&format!(
        "SELECT {} FROM {} ORDER BY {}",
        quoted.join(", "),
        table.name,
        table.key.join(", ")
    ))?;
    let placeholders: Vec<String> = (1..=all.len()).map(|n| format!("?{n}")).collect();
    let updates: Vec<String> = table
        .columns
        .iter()
        .map(|c| format!("\"{c}\" = excluded.\"{c}\""))
        .collect();
    let differs: Vec<String> = table
        .columns
        .iter()
        .map(|c| format!("\"{c}\" IS NOT excluded.\"{c}\""))
        .collect();
    let on_conflict = match table.existing {
        Existing::IfNewer => format!(
            "DO UPDATE SET {} WHERE excluded.updated_at > {}.updated_at AND ({})",
            updates.join(", "),
            table.name,
            differs.join(" OR ")
        ),
        Existing::Keep | Existing::AppendComment => "DO NOTHING".to_owned(),
    };
    let mut upsert = hub.prepare(&format!(
        "INSERT INTO {} ({}) VALUES ({})
         ON CONFLICT({}) {}",
        table.name,
        quoted.join(", "),
        placeholders.join(", "),
        table.key.join(", "),
        on_conflict
    ))?;
    let mut rows = select.query([])?;
    while let Some(row) = rows.next()? {
        let mut values = (0..all.len())
            .map(|i| row.get::<_, Value>(i))
            .collect::<rusqlite::Result<Vec<Value>>>()?;
        if table.name == "edges" && values[0] == values[1] {
            let count = report.kind(table.kind);
            count.read += 1;
            count.skipped += 1;
            if let Value::Text(id) = &values[0] {
                report.problem(format!("{id} waits on itself; that edge was left out"));
            }
            continue;
        }
        if table.name == "tickets"
            && let Some(labels) = all.iter().position(|c| *c == "labels")
        {
            values[labels] = without_roles(&values[labels]);
        }
        let mut changed = upsert.execute(params_from_iter(values.iter()))?;
        if changed == 0 && table.existing == Existing::AppendComment {
            changed = append_comment(hub, &values, report)?;
        }
        let count = report.kind(table.kind);
        count.read += 1;
        if changed > 0 {
            count.written += 1;
        } else {
            count.skipped += 1;
        }
    }
    Ok(())
}

/// A ticket's `labels` (a JSON array of strings) without its `role:*` labels. A value that is
/// not such an array is kept as it is.
fn without_roles(labels: &Value) -> Value {
    let Value::Text(text) = labels else {
        return labels.clone();
    };
    let Ok(list) = serde_json::from_str::<Vec<String>>(text) else {
        return labels.clone();
    };
    let kept: Vec<String> = list
        .into_iter()
        .filter(|label| !label.starts_with("role:"))
        .collect();
    Value::Text(serde_json::to_string(&kept).unwrap_or_else(|_| text.clone()))
}

/// Appends an imported comment whose `(ticket, n)` the hub holds with another text, as the
/// ticket's next comment. Returns 1 if it was appended, 0 if the hub's comment is the same or
/// the ticket already has the imported text. `values` are the comment's key columns, then
/// `author`, `at`, `text` and `legacy_id`.
fn append_comment(hub: &Connection, values: &[Value], report: &mut ImportReport) -> Result<usize> {
    let (Value::Text(ticket), Value::Integer(n)) = (&values[0], &values[1]) else {
        return Ok(0);
    };
    let already: i64 = hub.query_row(
        "SELECT count(*) FROM comments WHERE ticket = ?1 AND text = ?2 AND author = ?3 AND at = ?4",
        params_from_iter([&values[0], &values[4], &values[2], &values[3]]),
        |row| row.get(0),
    )?;
    if already > 0 {
        return Ok(0);
    }
    hub.execute(
        "INSERT INTO comments (ticket, n, author, at, text, legacy_id)
         SELECT ?1, COALESCE(MAX(n), 0) + 1, ?2, ?3, ?4, ?5 FROM comments WHERE ticket = ?1",
        params_from_iter([&values[0], &values[2], &values[3], &values[4], &values[5]]),
    )?;
    report.problem(format!(
        "comment {n} of {ticket} differs from the hub's comment {n}; the imported one was \
         appended as a new comment and the hub's was kept"
    ));
    Ok(1)
}

/// Moves the hub's `seq` up to the old service's, which it stored as text that may read
/// `1419.0`.
fn raise_seq(old: &Connection, hub: &Connection, report: &mut ImportReport) -> Result<()> {
    let text: Option<String> = old
        .query_row("SELECT value FROM meta WHERE key = 'seq'", [], |row| {
            row.get(0)
        })
        .optional()?;
    let count = report.kind("seq");
    count.read += 1;
    let Some(old_seq) = text.as_deref().and_then(|t| t.parse::<f64>().ok()) else {
        count.skipped += 1;
        report.problem(format!("the old meta.seq is {text:?}, not a number"));
        return Ok(());
    };
    let old_seq = old_seq.max(0.0) as u64;
    if old_seq > feed::current(hub)?.0 {
        feed::set_current(hub, Seq(old_seq))?;
        count.written += 1;
    } else {
        count.skipped += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;
    use crate::import::{Sources, run};

    /// The old service's schema, as its `contract/schema.sql` declares it.
    const OLD_SCHEMA: &str = "
        CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
        CREATE TABLE tickets (
          id TEXT PRIMARY KEY, parent TEXT, next_child INTEGER NOT NULL DEFAULT 1,
          title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', acceptance TEXT NOT NULL DEFAULT '',
          design TEXT NOT NULL DEFAULT '', notes TEXT NOT NULL DEFAULT '',
          type TEXT NOT NULL DEFAULT 'task', status TEXT NOT NULL DEFAULT 'open',
          priority INTEGER NOT NULL DEFAULT 2, assignee TEXT, last INTEGER NOT NULL DEFAULT 0,
          labels TEXT NOT NULL DEFAULT '[]', metadata TEXT NOT NULL DEFAULT '{}',
          defer_until TEXT, close_reason TEXT, created_at TEXT NOT NULL, created_by TEXT NOT NULL,
          updated_at TEXT NOT NULL, closed_at TEXT, version INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE edges (src TEXT NOT NULL, dst TEXT NOT NULL,
          kind TEXT NOT NULL DEFAULT 'blocks', PRIMARY KEY (src, dst)) WITHOUT ROWID;
        CREATE TABLE comments (ticket TEXT NOT NULL, n INTEGER NOT NULL, author TEXT NOT NULL,
          at TEXT NOT NULL, text TEXT NOT NULL, legacy_id TEXT,
          PRIMARY KEY (ticket, n)) WITHOUT ROWID;
        CREATE TABLE history (ticket TEXT NOT NULL, seq INTEGER NOT NULL, actor TEXT NOT NULL,
          at TEXT NOT NULL, change TEXT NOT NULL, PRIMARY KEY (ticket, seq)) WITHOUT ROWID;
        CREATE TABLE leases (name TEXT PRIMARY KEY, holder TEXT NOT NULL, machine TEXT NOT NULL,
          expires_at TEXT NOT NULL, payload TEXT NOT NULL DEFAULT '{}',
          waiters TEXT NOT NULL DEFAULT '[]') WITHOUT ROWID;
        CREATE TABLE events (seq INTEGER PRIMARY KEY, at TEXT NOT NULL, kind TEXT NOT NULL,
          machine TEXT NOT NULL, actor TEXT NOT NULL, data TEXT NOT NULL);
        CREATE TABLE aliases (alias TEXT PRIMARY KEY, id TEXT NOT NULL) WITHOUT ROWID;
    ";

    /// Writes an old database with an epic, two children, an edge, comments, history, an
    /// alias, an event and a lease, at a fresh path.
    pub(crate) fn old_database(name: &str) -> std::path::PathBuf {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-import-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        let _ = std::fs::remove_file(&path);
        let old = Connection::open(&path).unwrap();
        old.execute_batch(OLD_SCHEMA).unwrap();
        old.execute_batch(
            "INSERT INTO meta VALUES ('seq', '41.0');
             INSERT INTO tickets (id, parent, next_child, title, type, status, labels,
                                  created_at, created_by, updated_at, version)
               VALUES ('ab1', NULL, 3, 'Epic', 'epic', 'open', '[]',
                       '2026-11-01T00:00:00.000Z', 'tk', '2026-11-01T00:00:00.000Z', 3),
                      ('ab1.1', 'ab1', 1, 'First', 'task', 'closed', '[\"role:executor\"]',
                       '2026-11-01T00:00:01.000Z', 'tk', '2026-11-02T00:00:00.000Z', 40),
                      ('ab1.2', 'ab1', 1, 'Second', 'task', 'open', '[]',
                       '2026-11-01T00:00:02.000Z', 'tk', '2026-11-01T00:00:02.000Z', 5);
             UPDATE tickets SET close_reason = 'done', closed_at = '2026-11-02T00:00:00.000Z'
               WHERE id = 'ab1.1';
             INSERT INTO edges VALUES ('ab1.2', 'ab1.1', 'blocks');
             INSERT INTO comments VALUES ('ab1.1', 1, 'tk', '2026-11-01T01:00:00.000Z', 'hi', NULL),
                                         ('ab1.1', 2, 'tk', '2026-11-01T02:00:00.000Z', 'bye', 'c-9');
             INSERT INTO history VALUES ('ab1.1', 40, 'tk', '2026-11-02T00:00:00.000Z',
                                         '{\"status\":[\"open\",\"closed\"]}');
             INSERT INTO aliases VALUES ('by-old1', 'ab1.1');
             INSERT INTO events VALUES (1, '2026-11-01T00:00:00.000Z', 'ci', 'm', 'a', '{}');
             INSERT INTO leases VALUES ('staging', 'a', 'm', '2026-11-01T00:00:00.000Z', '{}', '[]');",
        )
        .unwrap();
        path
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    fn import_tickets(hub: &mut Connection, path: &Path, dry_run: bool) -> ImportReport {
        let sources = Sources {
            tickets: Some(path.to_owned()),
            ..Sources::default()
        };
        run(hub, &sources, dry_run).unwrap().remove(0)
    }

    #[test]
    fn every_ticket_and_its_rows_arrive_and_a_second_import_writes_nothing() {
        let path = old_database("twice");
        let mut hub = hub::open_in_memory().unwrap();
        let first = import_tickets(&mut hub, &path, false);
        assert_eq!(first.get("tickets").unwrap().written, 3);
        for (table, rows) in [
            ("tickets", 3),
            ("edges", 1),
            ("comments", 2),
            ("history", 1),
            ("aliases", 1),
            ("leases", 0),
        ] {
            assert_eq!(count(&hub, table), rows, "{table}");
        }
        assert_eq!(first.get("events").unwrap().skipped, 1);
        assert_eq!(first.get("leases").unwrap().skipped, 1);
        assert_eq!(feed::current(&hub).unwrap(), Seq(41));

        let ticket = hub::tickets::load(&hub, "ab1.1").unwrap().unwrap();
        assert_eq!(ticket.title, "First");
        assert_eq!(ticket.close_reason.as_deref(), Some("done"));
        // The old service's role label is left out.
        assert!(ticket.labels.is_empty(), "{:?}", ticket.labels);
        let by_alias = hub::tickets::resolve(&hub, "by-old1").unwrap().unwrap();
        assert_eq!(by_alias.as_str(), "ab1.1");

        let second = import_tickets(&mut hub, &path, false);
        assert!(!second.wrote(), "{second}");
        assert_eq!(second.get("comments").unwrap().skipped, 2);
    }

    #[test]
    fn an_edge_from_a_ticket_to_itself_is_left_out() {
        let path = old_database("self-edge");
        Connection::open(&path)
            .unwrap()
            .execute("INSERT INTO edges VALUES ('ab1.2', 'ab1.2', 'blocks')", [])
            .unwrap();
        let mut hub = hub::open_in_memory().unwrap();
        let report = import_tickets(&mut hub, &path, false);
        assert_eq!(count(&hub, "edges"), 1);
        let edges = report.get("edges").unwrap();
        assert_eq!((edges.read, edges.written, edges.skipped), (2, 1, 1));
        assert!(
            report.to_string().contains("ab1.2 waits on itself"),
            "{report}"
        );
    }

    #[test]
    fn a_changed_old_row_is_written_again() {
        let path = old_database("changed");
        let mut hub = hub::open_in_memory().unwrap();
        import_tickets(&mut hub, &path, false);
        Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE tickets SET title = 'Renamed', updated_at = '2026-11-03T00:00:00.000Z'
                 WHERE id = 'ab1.2'",
                [],
            )
            .unwrap();
        let again = import_tickets(&mut hub, &path, false);
        assert_eq!(again.get("tickets").unwrap().written, 1);
        assert_eq!(
            hub::tickets::load(&hub, "ab1.2").unwrap().unwrap().title,
            "Renamed"
        );
    }

    #[test]
    fn a_rerun_after_the_hub_went_live_keeps_newer_hub_data() {
        let path = old_database("live");
        let mut hub = hub::open_in_memory().unwrap();
        import_tickets(&mut hub, &path, false);
        // The hub changes a ticket, adds an edge kind and a comment that shares an imported key.
        hub.execute(
            "UPDATE tickets SET title = 'Hub title', status = 'closed',
               updated_at = '2026-11-20T00:00:00.000Z' WHERE id = 'ab1.2'",
            [],
        )
        .unwrap();
        hub.execute("UPDATE edges SET kind = 'waits-for'", [])
            .unwrap();
        hub.execute(
            "UPDATE comments SET text = 'hub text' WHERE ticket = 'ab1.1' AND n = 2",
            [],
        )
        .unwrap();
        let again = import_tickets(&mut hub, &path, false);
        let ticket = hub::tickets::load(&hub, "ab1.2").unwrap().unwrap();
        assert_eq!(ticket.title, "Hub title");
        assert_eq!(
            ticket.status,
            ignite_core::protocol::tickets::Status::Closed
        );
        let kind: String = hub
            .query_row("SELECT kind FROM edges", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kind, "waits-for");
        let texts: Vec<String> = hub
            .prepare("SELECT text FROM comments WHERE ticket = 'ab1.1' ORDER BY n")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(texts, ["hi", "hub text", "bye"]);
        assert!(again.to_string().contains("appended"), "{again}");
        // Running it once more adds nothing.
        import_tickets(&mut hub, &path, false);
        assert_eq!(count(&hub, "comments"), 3);
    }

    #[test]
    fn a_dry_run_writes_nothing_but_reports_what_it_would() {
        let path = old_database("dry");
        let mut hub = hub::open_in_memory().unwrap();
        let report = import_tickets(&mut hub, &path, true);
        assert_eq!(report.get("tickets").unwrap().written, 3);
        assert_eq!(count(&hub, "tickets"), 0);
        assert_eq!(count(&hub, "comments"), 0);
        assert_eq!(feed::current(&hub).unwrap(), Seq(0));
    }

    #[test]
    fn a_missing_database_is_not_found_and_not_created() {
        let mut hub = hub::open_in_memory().unwrap();
        let path = ignite_core::test_files::temp_dir().join("ignite-import-no-such.db");
        let sources = Sources {
            tickets: Some(path.clone()),
            ..Sources::default()
        };
        assert!(matches!(
            run(&mut hub, &sources, false),
            Err(crate::StoreError::NotFound(_))
        ));
        assert!(!path.exists());
    }
}
