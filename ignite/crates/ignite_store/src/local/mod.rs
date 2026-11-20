//! A daemon's local database. A restarted daemon reads its state from here, so a restart loses
//! nothing.

pub mod activity;
pub mod builds;
pub mod dispatch_queue;
pub mod express_passes;
pub mod gate_memory;
pub mod grants;
pub mod handle_stops;
pub mod handles;
pub mod listings;
pub mod outbox;
pub mod passes;
pub mod pool;
pub mod results;
pub mod rows;
pub mod shown;
pub mod tip_state;
pub mod trees;
pub mod verdicts;

use std::path::Path;

use rusqlite::Connection;

use crate::Result;
use crate::migrate::{self, Migration};

/// The local schema, oldest step first. A later change appends a step; it never edits one. A
/// step that is not purely additive carries a `-- breaking:` line (see `migrate`).
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("schema.sql"),
        code: None,
    },
    Migration {
        version: 2,
        sql: include_str!("schema_2.sql"),
        code: None,
    },
    Migration {
        version: 3,
        sql: include_str!("schema_3.sql"),
        code: None,
    },
    Migration {
        version: 4,
        sql: include_str!("schema_4.sql"),
        code: None,
    },
    Migration {
        version: 5,
        sql: include_str!("schema_5.sql"),
        code: None,
    },
    Migration {
        version: 6,
        sql: include_str!("schema_6.sql"),
        code: None,
    },
    Migration {
        version: 7,
        sql: include_str!("schema_7.sql"),
        code: None,
    },
    Migration {
        version: 8,
        sql: include_str!("schema_8.sql"),
        code: None,
    },
    Migration {
        version: 9,
        sql: include_str!("schema_9.sql"),
        code: None,
    },
    Migration {
        version: 10,
        sql: include_str!("schema_10.sql"),
        code: None,
    },
    Migration {
        version: 11,
        sql: include_str!("schema_11.sql"),
        code: None,
    },
    Migration {
        version: 12,
        sql: include_str!("schema_12.sql"),
        code: None,
    },
    Migration {
        version: 13,
        sql: include_str!("schema_13.sql"),
        code: None,
    },
    Migration {
        version: 14,
        sql: include_str!("schema_14.sql"),
        code: None,
    },
    Migration {
        version: 15,
        sql: include_str!("schema_15.sql"),
        code: None,
    },
    Migration {
        version: 16,
        sql: include_str!("schema_16.sql"),
        code: None,
    },
    Migration {
        version: 17,
        sql: include_str!("schema_17.sql"),
        code: None,
    },
    Migration {
        version: 18,
        sql: include_str!("schema_18.sql"),
        code: None,
    },
];

pub fn open(path: &Path) -> Result<Connection> {
    migrate::open(path, MIGRATIONS)
}

/// Opens the daemon's existing database for reading only, with no migration (see
/// [`migrate::open_read_only`]).
pub fn open_read_only(path: &Path) -> Result<Connection> {
    migrate::open_read_only(path)
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
    fn every_migration_is_additive_unless_it_says_otherwise() {
        migrate::assert_additive_unless_breaking(MIGRATIONS);
        // Only the outbox rebuild and the rewrite of ticket IDs into capitals are breaking, so
        // a rollback to a release between them, or after the second, still opens.
        let breaking: Vec<u32> = MIGRATIONS
            .iter()
            .filter(|m| m.breaking())
            .map(|m| m.version)
            .collect();
        assert_eq!(breaking, [2, 13]);
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

    /// A database from before trees had owners keeps its trees, each with no owner.
    #[test]
    fn upgrading_to_owned_trees_keeps_every_tree() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate::apply(&mut conn, &MIGRATIONS[..6]).unwrap();
        conn.execute_batch(
            "INSERT INTO trees (id, path, kind, state, ticket) \
             VALUES ('landing-1', '/trees/landing-1', 'landing', 'landing', 'm:T.1');",
        )
        .unwrap();
        migrate::apply(&mut conn, MIGRATIONS).unwrap();
        let tree = trees::get(&conn, "landing-1").unwrap().unwrap();
        assert_eq!(
            (tree.state.as_str(), tree.ticket.as_deref(), tree.owner),
            ("landing", Some("m:T.1"), None)
        );
    }
}
