//! The migration runner. A database's schema is a list of numbered SQL migrations; the runner
//! applies, in one transaction each, those above the version recorded in `meta` under
//! `schema_version`, and records the new version. The runner creates `meta` itself, so no
//! migration does.
//!
//! A database also records, in `meta` under `min_reader_version`, the lowest schema version a
//! build must know to use it. An additive migration (only new tables, new indexes and new
//! columns that are nullable or have a default) leaves that value alone, so an older build,
//! such as the one a rollback restores, still opens the database and ignores what it does not
//! know. A migration that is not additive carries a [`BREAKING`] line in its SQL, which raises
//! the value to the migration's own version. Every query must name its columns, so that a
//! column an older build does not know cannot shift the ones it does.

use rusqlite::{Connection, OptionalExtension};

use crate::{Result, StoreError};

/// One step of a schema.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    /// Numbered from 1, without gaps.
    pub version: u32,
    pub sql: &'static str,
    /// Run after `sql`, in the same transaction, for a rewrite that SQL cannot express, such
    /// as finding ticket IDs inside free text. Its migration's SQL still decides whether it is
    /// breaking, so a migration with code carries a [`BREAKING`] line when the code is.
    pub code: Option<fn(&rusqlite::Transaction<'_>) -> Result<()>>,
}

/// A line a migration's SQL carries when it is not additive (it drops, renames or rewrites
/// something an older build reads). It raises `min_reader_version` to the migration's version.
pub const BREAKING: &str = "-- breaking:";

const META: &str = "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) \
                    WITHOUT ROWID";

/// The version recorded in `meta`, or 0 for a new database.
pub fn version(conn: &Connection) -> Result<u32> {
    conn.execute_batch(META)?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match value {
        None => Ok(0),
        Some(text) => text
            .parse()
            .map_err(|_| StoreError::Invalid(format!("meta.schema_version is {text:?}"))),
    }
}

impl Migration {
    /// Whether this migration is not additive, so that a build older than it cannot use the
    /// database afterwards.
    #[must_use]
    pub fn breaking(&self) -> bool {
        self.sql
            .lines()
            .any(|line| line.trim().starts_with(BREAKING))
    }
}

/// The `min_reader_version` of a database after `migrations[..version]` have been applied: the
/// version of the last breaking migration among them, or 1.
#[must_use]
pub fn min_reader_version(migrations: &[Migration], version: u32) -> u32 {
    migrations
        .iter()
        .take(version as usize)
        .filter(|migration| migration.breaking())
        .map(|migration| migration.version)
        .next_back()
        .unwrap_or(1)
}

/// The `min_reader_version` recorded in `meta`, if any.
fn recorded_min_reader(conn: &Connection) -> Result<Option<u32>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'min_reader_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    value
        .map(|text| {
            text.parse()
                .map_err(|_| StoreError::Invalid(format!("meta.min_reader_version is {text:?}")))
        })
        .transpose()
}

fn record(conn: &Connection, key: &str, value: u32) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value.to_string()],
    )?;
    Ok(())
}

/// Set once this process has said that it opened a newer database, so that the many
/// connections a daemon opens print the line once.
static SAID_NEWER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Applies every migration above the database's version and returns the database's version.
///
/// A database newer than the last migration is opened as it is, applying nothing, when its
/// `min_reader_version` is at most the last migration's version: the migrations it has that
/// this code lacks were additive. Otherwise, or when the database does not record the value
/// (a build from before the field wrote it), it is refused, since this code does not know the
/// tables it needs.
pub fn apply(conn: &mut Connection, migrations: &[Migration]) -> Result<u32> {
    for (index, migration) in migrations.iter().enumerate() {
        if migration.version as usize != index + 1 {
            return Err(StoreError::Invalid(format!(
                "migration {} is numbered {}",
                index + 1,
                migration.version
            )));
        }
    }
    let mut current = version(conn)?;
    let last = migrations.len() as u32;
    if current > last {
        let required = recorded_min_reader(conn)?.unwrap_or(current);
        if required > last {
            return Err(StoreError::Invalid(format!(
                "the database is at schema version {current} and needs a build that knows \
                 version {required}; this build's last migration is {last}"
            )));
        }
        if !SAID_NEWER.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!(
                "ignite: the database is at schema version {current}, newer than this build's \
                 {last}; its later migrations are additive, so it is opened as it is"
            );
        }
        return Ok(current);
    }
    for migration in &migrations[current as usize..] {
        let transaction = conn.transaction()?;
        transaction.execute_batch(migration.sql)?;
        if let Some(code) = migration.code {
            code(&transaction)?;
        }
        record(&transaction, "schema_version", migration.version)?;
        record(
            &transaction,
            "min_reader_version",
            min_reader_version(migrations, migration.version),
        )?;
        transaction.commit()?;
        current = migration.version;
    }
    if recorded_min_reader(conn)?.is_none() {
        // A database migrated before the value existed: it is as old as the code's list says.
        record(
            conn,
            "min_reader_version",
            min_reader_version(migrations, current),
        )?;
    }
    Ok(current)
}

/// Opens a database file with the settings every Ignite database uses, and migrates it.
pub fn open(path: &std::path::Path, migrations: &[Migration]) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    apply(&mut conn, migrations)?;
    Ok(conn)
}

/// Opens an in-memory database with the given schema, for tests.
pub fn open_in_memory(migrations: &[Migration]) -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    apply(&mut conn, migrations)?;
    Ok(conn)
}

/// Write-ahead logging so readers never wait for the writer, and a busy timeout so two
/// connections queue instead of failing.
fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(())
}

/// The reason a statement of `sql` is not additive, if any. Comments are ignored; a statement
/// is additive when it creates a table or an index, or adds a column that is nullable or has a
/// default.
#[cfg(test)]
pub(crate) fn non_additive_statement(sql: &str) -> Option<String> {
    let code: String = sql
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    for statement in code.split(';') {
        let words: Vec<String> = statement
            .split_whitespace()
            .map(str::to_ascii_uppercase)
            .collect();
        let text = words.join(" ");
        let additive = match words.first().map(String::as_str) {
            None => true,
            Some("CREATE") => {
                text.starts_with("CREATE TABLE ")
                    || text.starts_with("CREATE INDEX ")
                    || text.starts_with("CREATE UNIQUE INDEX ")
            }
            Some("ALTER") => {
                text.contains(" ADD COLUMN ")
                    && (!text.contains(" NOT NULL") || text.contains(" DEFAULT "))
            }
            Some(_) => false,
        };
        if !additive {
            return Some(statement.trim().to_string());
        }
    }
    None
}

/// Fails for a migration whose SQL is not additive and that does not say so with [`BREAKING`]
/// (version 1 creates the schema and is exempt), and for one that says so without need.
#[cfg(test)]
pub(crate) fn assert_additive_unless_breaking(migrations: &[Migration]) {
    for migration in migrations {
        let offender = non_additive_statement(migration.sql);
        if migration.version == 1 {
            continue;
        }
        assert!(
            offender.is_none() || migration.breaking(),
            "migration {} is not additive ({:?}); either make it additive or put a `{BREAKING}` \
             line in it, which makes older builds refuse the database",
            migration.version,
            offender.unwrap_or_default()
        );
        assert!(
            offender.is_some() || !migration.breaking(),
            "migration {} is additive but carries a `{BREAKING}` line",
            migration.version
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEPS: &[Migration] = &[
        Migration {
            version: 1,
            sql: "CREATE TABLE a (x INTEGER);",
            code: None,
        },
        Migration {
            version: 2,
            sql: "ALTER TABLE a ADD COLUMN y TEXT;",
            code: None,
        },
    ];

    #[test]
    fn applies_each_step_once_and_records_the_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(apply(&mut conn, &STEPS[..1]).unwrap(), 1);
        assert_eq!(apply(&mut conn, STEPS).unwrap(), 2);
        assert_eq!(apply(&mut conn, STEPS).unwrap(), 2);
        assert_eq!(version(&conn).unwrap(), 2);
        conn.execute("INSERT INTO a (x, y) VALUES (1, 'z')", [])
            .unwrap();
    }

    #[test]
    fn refuses_misnumbered_steps() {
        let gap = [Migration {
            version: 2,
            sql: "",
            code: None,
        }];
        assert!(apply(&mut Connection::open_in_memory().unwrap(), &gap).is_err());
    }

    #[test]
    fn a_failed_step_leaves_the_previous_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        let broken = [
            STEPS[0],
            Migration {
                version: 2,
                sql: "NOT SQL;",
                code: None,
            },
        ];
        assert!(apply(&mut conn, &broken).is_err());
        assert_eq!(version(&conn).unwrap(), 1);
    }

    /// A build that knows fewer migrations opens a database whose extra migrations were
    /// additive, as after a rollback, and keeps using the tables it knows.
    #[test]
    fn an_older_build_opens_an_additively_migrated_database() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, STEPS).unwrap();
        assert_eq!(apply(&mut conn, &STEPS[..1]).unwrap(), 2);
        assert_eq!(version(&conn).unwrap(), 2);
        conn.execute("INSERT INTO a (x) VALUES (1)", []).unwrap();
        conn.execute("SELECT x FROM a", []).ok();
    }

    /// A database whose newest migrations were not additive is refused by a build older than
    /// the last of them, and opened by one that knows it.
    #[test]
    fn an_older_build_refuses_a_database_with_a_breaking_migration() {
        let steps = [
            STEPS[0],
            STEPS[1],
            Migration {
                version: 3,
                sql: "-- breaking: rewrites a.\nDROP TABLE a;\nCREATE TABLE a (z TEXT);",
                code: None,
            },
            Migration {
                version: 4,
                sql: "CREATE TABLE b (x INTEGER);",
                code: None,
            },
        ];
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, &steps).unwrap();
        assert_eq!(recorded_min_reader(&conn).unwrap(), Some(3));
        assert!(apply(&mut conn, &steps[..2]).is_err());
        assert_eq!(apply(&mut conn, &steps[..3]).unwrap(), 4);
    }

    /// A database from a build that did not record `min_reader_version` is refused when newer,
    /// and gets the value from the migration list when it is current.
    #[test]
    fn a_database_without_the_value_is_refused_when_newer_and_backfilled_when_current() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, STEPS).unwrap();
        conn.execute("DELETE FROM meta WHERE key = 'min_reader_version'", [])
            .unwrap();
        assert!(apply(&mut conn, &STEPS[..1]).is_err());
        apply(&mut conn, STEPS).unwrap();
        assert_eq!(recorded_min_reader(&conn).unwrap(), Some(1));
    }

    #[test]
    fn upgrading_a_database_records_the_value() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, &STEPS[..1]).unwrap();
        apply(&mut conn, STEPS).unwrap();
        assert_eq!(recorded_min_reader(&conn).unwrap(), Some(1));
    }

    #[test]
    fn the_additive_test_tells_the_two_kinds_apart() {
        for sql in [
            "CREATE TABLE t (x INTEGER); CREATE INDEX i ON t (x);",
            "ALTER TABLE t ADD COLUMN y TEXT; -- DROP TABLE t;",
            "ALTER TABLE t ADD COLUMN y TEXT NOT NULL DEFAULT '';",
        ] {
            assert_eq!(non_additive_statement(sql), None, "{sql}");
        }
        for sql in [
            "DROP TABLE t;",
            "ALTER TABLE t RENAME TO u;",
            "ALTER TABLE t ADD COLUMN y TEXT NOT NULL;",
            "UPDATE t SET x = 1;",
            "INSERT INTO t (x) VALUES (1);",
            "CREATE TABLE u (x INTEGER); DROP INDEX i;",
        ] {
            assert!(non_additive_statement(sql).is_some(), "{sql}");
        }
    }
}
