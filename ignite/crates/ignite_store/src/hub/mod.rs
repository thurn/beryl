//! The hub's shared database. Each submodule owns the queries over its tables.

pub mod attachments;
pub mod batch;
pub mod capacity;
pub mod capital_ids;
pub mod check_state;
pub mod checks;
pub mod cycles;
pub mod edges;
pub mod efficiency;
pub mod emergency;
pub mod feed;
pub mod filer_deps;
pub mod findings;
pub mod fix_outcomes;
pub mod flaky_triage;
pub mod fleet;
pub mod fleet_settings;
pub mod leases;
pub mod listings;
pub mod machine_asks;
pub mod machine_snapshots;
mod meta;
pub mod metrics;
pub mod pins;
pub mod placement_deferrals;
pub mod provider_usage;
pub mod rates;
pub mod records;
pub mod rejected;
pub mod releases;
pub mod renumber;
pub mod results;
mod routing_migrations;
pub mod rows;
pub mod schedules;
pub mod session_titles;
pub mod shed;
pub mod ticket_spend;
pub mod tickets;
pub mod tip_coverage;
pub mod tip_misses;
pub mod tip_rounds;
pub mod tip_streaks;
pub mod tips;
pub mod tokens;
pub mod transcripts;
pub mod verdicts;

use std::path::Path;

use rusqlite::Connection;

use crate::Result;
use crate::migrate::{self, Migration};

/// The hub's schema, oldest step first. A later change appends a step; it never edits one. A
/// step that is not purely additive carries a `-- breaking:` line (see `migrate`).
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("schema.sql"),
        code: None,
    },
    Migration {
        version: 2,
        sql: include_str!("schema_metrics.sql"),
        code: None,
    },
    Migration {
        version: 3,
        sql: include_str!("feed.sql"),
        code: None,
    },
    Migration {
        version: 4,
        sql: include_str!("workers_command.sql"),
        code: None,
    },
    Migration {
        version: 5,
        sql: include_str!("dispatch_rules.sql"),
        code: None,
    },
    Migration {
        version: 6,
        sql: include_str!("flaky_details.sql"),
        code: None,
    },
    Migration {
        version: 7,
        sql: include_str!("rejected_commands.sql"),
        code: None,
    },
    Migration {
        version: 8,
        sql: include_str!("check_state_changes.sql"),
        code: None,
    },
    Migration {
        version: 9,
        sql: include_str!("listings.sql"),
        code: None,
    },
    Migration {
        version: 10,
        sql: include_str!("tip_rounds.sql"),
        code: None,
    },
    Migration {
        version: 11,
        sql: include_str!("two_budgets.sql"),
        code: None,
    },
    Migration {
        version: 12,
        sql: include_str!("capital_ids.sql"),
        code: Some(capital_ids::rewrite_text),
    },
    Migration {
        version: 13,
        sql: include_str!("session_titles.sql"),
        code: None,
    },
    Migration {
        version: 14,
        sql: include_str!("owners.sql"),
        code: Some(routing_migrations::retire_labels),
    },
    Migration {
        version: 15,
        sql: include_str!("ticket_spend.sql"),
        code: None,
    },
    Migration {
        version: 16,
        sql: include_str!("schedules.sql"),
        code: None,
    },
    Migration {
        version: 17,
        sql: include_str!("tip_misses.sql"),
        code: None,
    },
    Migration {
        version: 18,
        sql: include_str!("listing_alone.sql"),
        code: None,
    },
    Migration {
        version: 19,
        sql: include_str!("hierarchical_ids.sql"),
        code: Some(renumber::renumber_all),
    },
    Migration {
        version: 20,
        sql: include_str!("workers_state_since.sql"),
        code: None,
    },
    Migration {
        version: 21,
        sql: include_str!("verdicts.sql"),
        code: None,
    },
    Migration {
        version: 22,
        sql: include_str!("metrics_kinds.sql"),
        code: Some(metrics::prune_and_fill_kinds),
    },
    Migration {
        version: 23,
        sql: include_str!("metrics_indexes.sql"),
        code: None,
    },
    Migration {
        version: 24,
        sql: include_str!("ticket_releases.sql"),
        code: None,
    },
    Migration {
        version: 25,
        sql: include_str!("transcripts.sql"),
        code: None,
    },
    Migration {
        version: 26,
        sql: include_str!("transcript_calls.sql"),
        code: None,
    },
    Migration {
        version: 27,
        sql: include_str!("attachments.sql"),
        code: None,
    },
    Migration {
        version: 28,
        sql: include_str!("transcripts_by_ticket.sql"),
        code: None,
    },
    Migration {
        version: 29,
        sql: include_str!("transcript_sessions.sql"),
        code: None,
    },
    Migration {
        version: 30,
        sql: include_str!("machine_cli_apps.sql"),
        code: None,
    },
    Migration {
        version: 31,
        sql: include_str!("reservation_landings.sql"),
        code: None,
    },
    Migration {
        version: 32,
        sql: include_str!("fleet_settings.sql"),
        code: None,
    },
    Migration {
        version: 33,
        sql: include_str!("machine_asks.sql"),
        code: None,
    },
    Migration {
        version: 34,
        sql: "",
        code: Some(routing_migrations::owners_to_pins),
    },
    Migration {
        version: 35,
        sql: include_str!("reservation_providers.sql"),
        code: None,
    },
    Migration {
        version: 36,
        sql: include_str!("regression_verification.sql"),
        code: None,
    },
    Migration {
        version: 37,
        sql: include_str!("fix_outcomes.sql"),
        code: None,
    },
    Migration {
        version: 38,
        sql: include_str!("tip_failure.sql"),
        code: None,
    },
    Migration {
        version: 39,
        sql: include_str!("reservation_master_blocker.sql"),
        code: None,
    },
    Migration {
        version: 40,
        sql: include_str!("invalid_checks.sql"),
        code: None,
    },
    Migration {
        version: 41,
        sql: include_str!("machine_snapshots.sql"),
        code: None,
    },
    Migration {
        version: 42,
        sql: include_str!("work_estimates.sql"),
        code: None,
    },
    Migration {
        version: 43,
        sql: "ALTER TABLE machine_snapshots ADD COLUMN capacity TEXT;",
        code: None,
    },
    Migration {
        version: 44,
        sql: "ALTER TABLE machine_asks ADD COLUMN no_ticket_reason TEXT;",
        code: None,
    },
    Migration {
        version: 45,
        sql: include_str!("tree_volumes.sql"),
        code: None,
    },
    Migration {
        version: 46,
        sql: "ALTER TABLE tickets ADD COLUMN allocation INTEGER;",
        code: Some(tickets::backfill_allocation),
    },
    Migration {
        version: 47,
        sql: "",
        code: Some(fleet_settings::migrate_disk_cap),
    },
    Migration {
        version: 48,
        sql: "ALTER TABLE machine_snapshots ADD COLUMN last_disk_cleanup_at TEXT;",
        code: None,
    },
    Migration {
        version: 49,
        sql: "ALTER TABLE machine_snapshots ADD COLUMN active_memory_pressure INTEGER;",
        code: None,
    },
    Migration {
        version: 50,
        sql: "ALTER TABLE machine_snapshots ADD COLUMN fseventsd_rss_mib INTEGER;",
        code: None,
    },
    Migration {
        version: 51,
        sql: "",
        code: Some(routing_migrations::backfill_os_requirements),
    },
    Migration {
        version: 52,
        sql: "ALTER TABLE tips ADD COLUMN observed_failures TEXT;",
        code: None,
    },
    Migration {
        version: 53,
        sql: "ALTER TABLE regressions ADD COLUMN landing_attribution TEXT;",
        code: None,
    },
    Migration {
        version: 54,
        sql: include_str!("metrics_ingest.sql"),
        code: Some(metrics::number_existing_rows),
    },
    Migration {
        version: 55,
        sql: include_str!("external_edges.sql"),
        code: None,
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
    migrate::open_read_only(path)
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
    fn every_migration_is_additive_unless_it_says_otherwise() {
        migrate::assert_additive_unless_breaking(MIGRATIONS);
        // Only the rewrite of ticket IDs into capitals is breaking, so a rollback to a release
        // after it still opens.
        let breaking: Vec<u32> = MIGRATIONS
            .iter()
            .filter(|m| m.breaking())
            .map(|m| m.version)
            .collect();
        assert_eq!(breaking, [12]);
    }

    /// `select` names the live columns. The retired owner column stays in the schema.
    #[test]
    fn the_ticket_column_list_matches_the_live_columns() {
        let conn = open_in_memory().unwrap();
        let actual: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('tickets') WHERE name != 'owner'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        let listed: Vec<&str> = tickets::TICKET_COLUMNS.split(',').map(str::trim).collect();
        assert_eq!(actual, listed);
    }

    #[test]
    fn the_row_structs_match_the_schema() {
        let conn = open_in_memory().unwrap();
        crate::test_support::assert_rows_match_schema(&conn, rows::TABLES);
    }

    #[test]
    fn a_read_only_open_neither_migrates_nor_writes() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-hub-ro-{}", std::process::id()));
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
        let dir =
            ignite_core::test_files::temp_dir().join(format!("ignite-hub-{}", std::process::id()));
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
