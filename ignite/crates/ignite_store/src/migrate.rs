//! The migration runner. A database's schema is a list of numbered SQL migrations; the runner
//! applies, in one transaction each, those above the version recorded in `meta` under
//! `schema_version`, and records the new version. The runner creates `meta` itself, so no
//! migration does.

use rusqlite::{Connection, OptionalExtension};

use crate::{Result, StoreError};

/// One step of a schema.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    /// Numbered from 1, without gaps.
    pub version: u32,
    pub sql: &'static str,
}

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

/// Applies every migration above the database's version and returns the new version. A
/// database newer than the last migration is refused, since this code does not know its tables.
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
        return Err(StoreError::Invalid(format!(
            "the database is at schema version {current}, newer than this build's {last}"
        )));
    }
    for migration in &migrations[current as usize..] {
        let transaction = conn.transaction()?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [migration.version.to_string()],
        )?;
        transaction.commit()?;
        current = migration.version;
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

#[cfg(test)]
mod tests {
    use super::*;

    const STEPS: &[Migration] = &[
        Migration {
            version: 1,
            sql: "CREATE TABLE a (x INTEGER);",
        },
        Migration {
            version: 2,
            sql: "ALTER TABLE a ADD COLUMN y TEXT;",
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
    fn refuses_a_newer_database_and_misnumbered_steps() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn, STEPS).unwrap();
        assert!(matches!(
            apply(&mut conn, &STEPS[..1]),
            Err(StoreError::Invalid(_))
        ));
        let gap = [Migration {
            version: 2,
            sql: "",
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
            },
        ];
        assert!(apply(&mut conn, &broken).is_err());
        assert_eq!(version(&conn).unwrap(), 1);
    }
}
