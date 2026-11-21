//! Check family listings that machines share, by family and listing hash. The hub keeps each
//! listing as the text a machine sent and never reads it.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// How many listings of one family are kept; older ones are forgotten. Machines on different
/// platforms and branches store their own, so this is more than one machine keeps.
pub const KEPT: i64 = 48;

/// The listing stored for `family` under `hash`, if any.
pub fn get(conn: &Connection, family: &str, hash: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT listing FROM listings WHERE family = ?1 AND hash = ?2",
            params![family, hash],
            |row| row.get(0),
        )
        .optional()?)
}

/// Whether the key is stored, without loading the listing.
pub fn contains(conn: &Connection, family: &str, hash: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM listings WHERE family = ?1 AND hash = ?2)",
        params![family, hash],
        |row| row.get(0),
    )?)
}

/// Stores a listing `machine` made, replacing one under the same family and hash, and forgets
/// all but the [`KEPT`] newest listings of the family.
pub fn put(
    conn: &Connection,
    family: &str,
    hash: &str,
    listing: &str,
    machine: &str,
    at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO listings (family, hash, listing, machine, at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![family, hash, listing, machine, at],
    )?;
    conn.execute(
        "DELETE FROM listings WHERE family = ?1 AND rowid NOT IN \
         (SELECT rowid FROM listings WHERE family = ?1 ORDER BY at DESC, rowid DESC LIMIT ?2)",
        params![family, KEPT],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;

    #[test]
    fn a_listing_is_shared_by_family_and_hash_and_old_ones_are_forgotten() {
        let conn = open_in_memory().unwrap();
        put(
            &conn,
            "unit",
            "h0",
            "[0]",
            "m-1",
            "2026-11-01T00:00:00.000Z",
        )
        .unwrap();
        assert_eq!(get(&conn, "unit", "h0").unwrap().as_deref(), Some("[0]"));
        assert_eq!(get(&conn, "unit", "h1").unwrap(), None);
        for n in 1..=KEPT {
            let at = format!("2026-11-02T00:{:02}:{:02}.000Z", n / 60, n % 60);
            put(&conn, "unit", &format!("h{n}"), "[n]", "m-2", &at).unwrap();
        }
        assert_eq!(
            get(&conn, "unit", "h0").unwrap(),
            None,
            "the oldest is gone"
        );
        assert!(get(&conn, "unit", &format!("h{KEPT}")).unwrap().is_some());
    }
}
