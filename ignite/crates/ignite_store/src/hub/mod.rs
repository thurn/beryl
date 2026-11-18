//! The hub's shared database. Each submodule owns the queries over its tables.

pub mod batch;
pub mod checks;
pub mod edges;
pub mod feed;
pub mod findings;
pub mod fleet;
pub mod leases;
pub mod records;
pub mod results;
pub mod rows;
pub mod tickets;
pub mod tips;
pub mod tokens;

use std::path::Path;

use rusqlite::Connection;

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
];

/// Opens (creating if needed) and migrates the hub's database.
pub fn open(path: &Path) -> Result<Connection> {
    migrate::open(path, MIGRATIONS)
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
