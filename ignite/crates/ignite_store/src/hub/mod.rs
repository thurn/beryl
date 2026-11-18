//! The hub's shared database. Each submodule owns the queries over its tables.

pub mod batch;
pub mod catches;
pub mod check_state;
pub mod checks;
pub mod cycles;
pub mod edges;
pub mod feed;
pub mod findings;
pub mod fleet;
pub mod leases;
pub mod records;
pub mod rejected;
pub mod results;
pub mod rows;
pub mod tickets;
pub mod tips;
pub mod tokens;

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::Result;
use crate::migrate::{self, Migration};

/// The hub's schema, oldest step first. A later change appends a step; it never edits one.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("schema.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("schema_metrics.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("feed.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("workers_command.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("dispatch_rules.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("flaky_details.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("rejected_commands.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("check_state_changes.sql"),
    },
];

/// Opens (creating if needed) and migrates the hub's database.
pub fn open(path: &Path) -> Result<Connection> {
    migrate::open(path, MIGRATIONS)
}

/// Opens an existing hub database for reading only: no migration, no write-ahead log setting,
/// nothing created. A tool run against the live hub's file, such as `ignite gate replay` built
/// from a newer commit, must not migrate it under the running hub.
pub fn open_read_only(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}

/// A migrated in-memory hub database, for tests.
pub fn open_in_memory() -> Result<Connection> {
    migrate::open_in_memory(MIGRATIONS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_database_is_at_the_last_version() {
        let conn = open_in_memory().unwrap();
        assert_eq!(migrate::version(&conn).unwrap(), MIGRATIONS.len() as u32);
    }

    #[test]
    fn the_row_structs_match_the_schema() {
        let conn = open_in_memory().unwrap();
        crate::test_support::assert_rows_match_schema(&conn, rows::TABLES);
    }

    #[test]
    fn a_read_only_open_neither_migrates_nor_writes() {
        let dir = std::env::temp_dir().join(format!("ignite-hub-ro-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.sqlite");
        let _ = std::fs::remove_file(&path);
        // A database one migration behind this build, as the live hub's is when a newer
        // binary reads it.
        let behind = MIGRATIONS.len() - 1;
        let mut old = rusqlite::Connection::open(&path).unwrap();
        migrate::apply(&mut old, &MIGRATIONS[..behind]).unwrap();
        drop(old);
        let conn = open_read_only(&path).unwrap();
        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, behind.to_string());
        assert!(
            conn.execute("INSERT INTO aliases VALUES ('a', 'b.1')", [])
                .is_err()
        );
        drop(conn);
        assert!(open_read_only(&dir.join("missing.sqlite")).is_err());
        assert!(!dir.join("missing.sqlite").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reopening_a_file_keeps_its_rows() {
        let dir = std::env::temp_dir().join(format!("ignite-hub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.sqlite");
        let _ = std::fs::remove_file(&path);
        open(&path)
            .unwrap()
            .execute("INSERT INTO aliases VALUES ('old', 'a.1')", [])
            .unwrap();
        let conn = open(&path).unwrap();
        let id: String = conn
            .query_row("SELECT id FROM aliases WHERE alias = 'old'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(id, "a.1");
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
