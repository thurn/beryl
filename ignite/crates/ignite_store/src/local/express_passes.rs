//! Express runs that passed, by the git tree they checked and the `[express]` section they ran.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// Where an express pass ran: the working tree, the commit it had checked out and the
/// change's own files on that commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub tree_path: String,
    pub head: String,
    pub changed: Vec<String>,
}

/// The last pass recorded for a working tree (see [`last_in`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastPass {
    pub at: String,
    pub place: Place,
}

/// Records that the express commands described by `config` passed on `tree_id` at `at`, in
/// `place` when the caller knows it, and forgets every pass from before `forget_before`.
pub fn record(
    conn: &Connection,
    tree_id: &str,
    config: &str,
    at: &str,
    forget_before: &str,
    place: Option<&Place>,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO express_passes (tree_id, config, at, tree_path, head, changed) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            tree_id,
            config,
            at,
            place.map(|p| p.tree_path.as_str()),
            place.map(|p| p.head.as_str()),
            place.map(|p| p.changed.join("\n")),
        ],
    )?;
    conn.execute("DELETE FROM express_passes WHERE at < ?1", [forget_before])?;
    Ok(())
}

/// When the commands described by `config` last passed on `tree_id`, if that was at or after
/// `since`.
pub fn passed_at(
    conn: &Connection,
    tree_id: &str,
    config: &str,
    since: &str,
) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT at FROM express_passes WHERE tree_id = ?1 AND config = ?2 AND at >= ?3",
            params![tree_id, config, since],
            |row| row.get(0),
        )
        .optional()?)
}

/// The newest pass of the commands described by `config` in the working tree at `tree_path`,
/// if one was recorded with its place at or after `since`.
pub fn last_in(
    conn: &Connection,
    tree_path: &str,
    config: &str,
    since: &str,
) -> Result<Option<LastPass>> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT at, head, changed FROM express_passes \
             WHERE tree_path = ?1 AND config = ?2 AND at >= ?3 \
               AND head IS NOT NULL AND changed IS NOT NULL \
             ORDER BY at DESC LIMIT 1",
            params![tree_path, config, since],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    Ok(row.map(|(at, head, changed)| LastPass {
        at,
        place: Place {
            tree_path: tree_path.to_owned(),
            head,
            changed: changed
                .lines()
                .filter(|file| !file.is_empty())
                .map(str::to_owned)
                .collect(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    #[test]
    fn a_pass_is_found_for_its_tree_and_section_until_it_expires() {
        let conn = open_in_memory().unwrap();
        record(
            &conn,
            "t1",
            "c1",
            "2026-11-18T10:00:00Z",
            "2026-11-18T09:00:00Z",
            None,
        )
        .unwrap();
        let since = "2026-11-18T10:00:00Z";
        assert!(passed_at(&conn, "t1", "c1", since).unwrap().is_some());
        assert_eq!(passed_at(&conn, "t2", "c1", since).unwrap(), None);
        assert_eq!(passed_at(&conn, "t1", "c2", since).unwrap(), None);
        assert_eq!(
            passed_at(&conn, "t1", "c1", "2026-11-18T10:00:01Z").unwrap(),
            None
        );
        // A later record forgets the expired one.
        record(
            &conn,
            "t2",
            "c1",
            "2026-11-18T12:00:00Z",
            "2026-11-18T11:00:00Z",
            None,
        )
        .unwrap();
        assert_eq!(
            passed_at(&conn, "t1", "c1", "2000-01-01T00:00:00Z").unwrap(),
            None
        );
    }

    #[test]
    fn the_last_pass_of_a_tree_is_found_by_its_path_with_its_commit_and_files() {
        let conn = open_in_memory().unwrap();
        let place = |head: &str| Place {
            tree_path: "/trees/tree-01".to_owned(),
            head: head.to_owned(),
            changed: vec!["ignite/a.rs".to_owned(), "ignite/b.rs".to_owned()],
        };
        let forget = "2026-11-18T09:00:00Z";
        record(
            &conn,
            "t1",
            "c1",
            "2026-11-18T10:00:00Z",
            forget,
            Some(&place("a")),
        )
        .unwrap();
        record(
            &conn,
            "t2",
            "c1",
            "2026-11-18T10:05:00Z",
            forget,
            Some(&place("b")),
        )
        .unwrap();
        record(&conn, "t3", "c1", "2026-11-18T10:09:00Z", forget, None).unwrap();
        let since = "2026-11-18T09:30:00Z";
        let last = last_in(&conn, "/trees/tree-01", "c1", since)
            .unwrap()
            .unwrap();
        assert_eq!(
            last.at, "2026-11-18T10:05:00Z",
            "a pass with no place is skipped"
        );
        assert_eq!(last.place, place("b"));
        assert_eq!(last_in(&conn, "/trees/tree-02", "c1", since).unwrap(), None);
        assert_eq!(last_in(&conn, "/trees/tree-01", "c2", since).unwrap(), None);
        let later = "2026-11-18T10:06:00Z";
        assert_eq!(last_in(&conn, "/trees/tree-01", "c1", later).unwrap(), None);
    }
}
