//! A daemon's local database. A restarted daemon reads its state from here, so a restart loses
//! nothing.

pub mod grants;
pub mod handles;
pub mod outbox;
pub mod pool;
pub mod results;
pub mod rows;
pub mod trees;

use std::path::Path;

use rusqlite::Connection;

use crate::Result;
use crate::migrate::{self, Migration};

/// The local schema, oldest step first. A later change appends a step; it never edits one.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("schema.sql"),
}];

pub fn open(path: &Path) -> Result<Connection> {
    migrate::open(path, MIGRATIONS)
}

pub fn open_in_memory() -> Result<Connection> {
    migrate::open_in_memory(MIGRATIONS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_structs_match_the_schema() {
        let conn = open_in_memory().unwrap();
        assert_eq!(migrate::version(&conn).unwrap(), MIGRATIONS.len() as u32);
        crate::test_support::assert_rows_match_schema(&conn, rows::TABLES);
    }
}
