//! The machine's copy of check family listings, by family and listing hash, used before the
//! hub is asked.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// How many listings of one family are kept; older ones are forgotten.
pub const KEPT: i64 = 16;

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

/// Stores a listing, replacing one under the same family and hash, and forgets all but the
/// [`KEPT`] newest listings of the family.
pub fn put(conn: &Connection, family: &str, hash: &str, listing: &str, at: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO listings (family, hash, listing, at) VALUES (?1, ?2, ?3, ?4)",
        params![family, hash, listing, at],
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
    use crate::local::open_in_memory;

    #[test]
    fn a_listing_is_found_by_family_and_hash_and_old_ones_are_forgotten() {
        let conn = open_in_memory().unwrap();
        put(&conn, "unit", "h0", "[0]", "2026-11-01T00:00:00.000Z").unwrap();
        assert_eq!(get(&conn, "unit", "h0").unwrap().as_deref(), Some("[0]"));
        assert_eq!(get(&conn, "unit", "h1").unwrap(), None);
        assert_eq!(get(&conn, "other", "h0").unwrap(), None);
        put(&conn, "other", "h0", "[x]", "2026-11-01T00:00:00.000Z").unwrap();
        for n in 1..=KEPT {
            let at = format!("2026-11-02T00:00:{n:02}.000Z");
            put(&conn, "unit", &format!("h{n}"), "[n]", &at).unwrap();
        }
        assert_eq!(
            get(&conn, "unit", "h0").unwrap(),
            None,
            "the oldest is gone"
        );
        assert!(get(&conn, "unit", &format!("h{KEPT}")).unwrap().is_some());
        assert!(get(&conn, "other", "h0").unwrap().is_some(), "per family");
    }
}
