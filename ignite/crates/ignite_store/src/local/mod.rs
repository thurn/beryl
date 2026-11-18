//! A daemon's local database. A restarted daemon reads its state from here, so a restart loses
//! nothing.

pub mod grants;
pub mod handles;
pub mod outbox;
pub mod passes;
pub mod pool;
pub mod results;
pub mod rows;
pub mod shown;
pub mod trees;

use std::path::Path;

use rusqlite::Connection;

use crate::Result;
use crate::migrate::{self, Migration};

/// The local schema, oldest step first. A later change appends a step; it never edits one.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("schema.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("schema_2.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("schema_3.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("schema_4.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("schema_5.sql"),
    },
];

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

    #[test]
    fn upgrading_from_version_one_keeps_trees_and_unsent_entries() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate::apply(&mut conn, &MIGRATIONS[..1]).unwrap();
        conn.execute_batch(
            "INSERT INTO trees (id, path, kind) VALUES ('tree-01', '/trees/tree-01', 'ticket');
             INSERT INTO outbox (kind, body, at) VALUES ('record', '{}', 't1'), ('result', '{}', 't2');",
        )
        .unwrap();
        migrate::apply(&mut conn, MIGRATIONS).unwrap();
        let tree = trees::get(&conn, "tree-01").unwrap().unwrap();
        assert_eq!(tree.state, "free");
        let seqs: Vec<i64> = outbox::peek(&conn, 10)
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect();
        assert_eq!(seqs, [1, 2]);
        assert_eq!(outbox::push(&conn, "record", "{}", "t3").unwrap(), 3);
    }
}
