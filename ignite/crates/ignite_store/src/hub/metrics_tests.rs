use rusqlite::{Connection, params};

use super::*;
use crate::hub::open_in_memory;
use crate::migrate;

fn insert(conn: &Connection, data: &str, at: &str) {
    let kind = serde_json::from_str::<serde_json::Value>(data)
        .ok()
        .and_then(|row| kind_of(&row).map(str::to_owned));
    conn.execute(
        "INSERT INTO metrics (machine, source, data, at, kind) VALUES ('m-1', 'host', ?1, ?2, ?3)",
        params![data, at, kind],
    )
    .unwrap();
}

fn count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn rows_older_than_the_retention_are_deleted_in_batches() {
    let conn = open_in_memory().unwrap();
    let now: Timestamp = "2026-12-31T00:00:00.000Z".parse().unwrap();
    let old = now.plus_seconds(-(KEEP_DAYS + 1.0) * 86_400.0).to_string();
    let recent = now.plus_seconds(-(KEEP_DAYS - 1.0) * 86_400.0).to_string();
    for n in 0..(DELETE_BATCH + 3) {
        insert(
            &conn,
            &format!(r#"{{"table":"tool_calls","row":{{"n":{n}}}}}"#),
            &old,
        );
    }
    insert(&conn, r#"{"rule":"r"}"#, &recent);
    assert_eq!(delete_expired(&conn, now).unwrap() as i64, DELETE_BATCH + 3);
    assert_eq!(count(&conn), 1);
    assert_eq!(delete_expired(&conn, now).unwrap(), 0);
}

/// Opens a database at the schema before `kind`, adds rows, and applies the rest.
#[test]
fn the_kind_migration_drops_rebuilt_rows_and_fills_kinds() {
    let mut conn = Connection::open_in_memory().unwrap();
    let migrations = crate::hub::MIGRATIONS;
    let before = migrations.iter().position(|m| m.version == 22).unwrap();
    migrate::apply(&mut conn, &migrations[..before]).unwrap();
    for data in [
        r#"{"export":1,"host":"h"}"#,
        r#"{"table":"compile_timings","row":{}}"#,
        r#"{"table":"ignite_records","row":{}}"#,
        r#"{"table":"leases","row":{}}"#,
        r#"{"table":"window_dollars","row":{}}"#,
    ] {
        conn.execute(
            "INSERT INTO metrics (machine, source, data, at) VALUES ('m-1', 'h', ?1, 'x')",
            params![data],
        )
        .unwrap();
    }
    migrate::apply(&mut conn, migrations).unwrap();
    let mut kinds: Vec<Option<String>> = conn
        .prepare("SELECT kind FROM metrics ORDER BY kind")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    kinds.sort();
    assert_eq!(kinds, vec![None, Some("window_dollars".to_owned())]);
}

#[test]
fn the_ingestion_migration_preserves_rows_and_never_reuses_a_cursor() {
    let mut conn = Connection::open_in_memory().unwrap();
    let migrations = crate::hub::MIGRATIONS;
    let before = migrations.iter().position(|m| m.version == 54).unwrap();
    migrate::apply(&mut conn, &migrations[..before]).unwrap();
    let at = "2026-11-24T00:00:00.000Z";
    let old = r#"{"table":"requests","row":{}}"#;
    let header = r#"{"export":1,"host":"host","schema":42}"#;
    insert(&conn, old, at);
    insert(&conn, header, at);
    migrate::apply(&mut conn, migrations).unwrap();

    let page = crate::hub::efficiency::metrics(&mut conn, 0, 10).unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.rows[0].data, header);
    assert_eq!(page.rows[1].data, old);
    assert_eq!(page.rows[1].at, at);
    assert_eq!(page.headers[0].data, header);
    let last_id = page.last_id;

    let now: Timestamp = "2026-12-31T00:00:00.000Z".parse().unwrap();
    assert_eq!(delete_expired(&conn, now).unwrap(), 2);
    assert_eq!(count(&conn), 0);
    // An older release can open the migrated database and insert with its old columns.
    migrate::apply(&mut conn, &migrations[..before]).unwrap();
    insert(&conn, old, at);
    let page = crate::hub::efficiency::metrics(&mut conn, last_id, 10).unwrap();
    assert!(page.last_id > last_id);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].data, old);
}
