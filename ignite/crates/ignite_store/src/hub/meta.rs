//! JSON values in the hub metadata table.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::{Result, StoreError};

pub(super) fn get<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    key: &str,
) -> Result<Option<T>> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    value
        .map(|json| {
            serde_json::from_str(&json).map_err(|e| StoreError::Invalid(format!("meta {key}: {e}")))
        })
        .transpose()
}

pub(super) fn put(conn: &Connection, key: &str, value: &impl Serialize) -> Result<()> {
    let json = serde_json::to_string(value)
        .map_err(|e| StoreError::Invalid(format!("meta {key}: {e}")))?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, json],
    )?;
    Ok(())
}
