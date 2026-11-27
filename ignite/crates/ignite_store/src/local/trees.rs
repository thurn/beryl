//! The machine's build trees: which are leased, to whom, and what each last built.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Result;
use crate::local::rows;

/// What a tree is doing. A tree is leased only from `Free`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Free,
    /// The daemon is building it at the tip, ahead of the next lease.
    Prebuilding,
    /// Leased to a ticket's session.
    Leased,
    /// Held by a landing or tip verification.
    Landing,
    /// Being reset to a clean checkout after its lease ended.
    Resetting,
    /// Marked for deletion, which the disk loop resumes after a restart.
    Retiring,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Free => "free",
            State::Prebuilding => "prebuilding",
            State::Leased => "leased",
            State::Landing => "landing",
            State::Resetting => "resetting",
            State::Retiring => "retiring",
        }
    }
}

const COLUMNS: &str = "id, path, kind, state, ticket, session, branch, built_commit, leased_at, \
                       last_used, owner";

fn tree(row: &Row) -> rusqlite::Result<rows::Tree> {
    Ok(rows::Tree {
        id: row.get(0)?,
        path: row.get(1)?,
        kind: row.get(2)?,
        state: row.get(3)?,
        ticket: row.get(4)?,
        session: row.get(5)?,
        branch: row.get(6)?,
        built_commit: row.get(7)?,
        leased_at: row.get(8)?,
        last_used: row.get(9)?,
        owner: row.get(10)?,
    })
}

pub fn all(conn: &Connection) -> Result<Vec<rows::Tree>> {
    let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM trees ORDER BY id"))?;
    let trees = statement
        .query_map([], tree)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(trees)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<rows::Tree>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM trees WHERE id = ?1"),
            [id],
            tree,
        )
        .optional()?)
}

/// Inserts the tree or replaces every column of the existing row.
pub fn upsert(conn: &Connection, tree: &rows::Tree) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO trees ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, \
             ?10, ?11)"
        ),
        params![
            tree.id,
            tree.path,
            tree.kind,
            tree.state,
            tree.ticket,
            tree.session,
            tree.branch,
            tree.built_commit,
            tree.leased_at,
            tree.last_used,
            tree.owner,
        ],
    )?;
    Ok(())
}

/// Moves a free tree to `to` (`Leased` for a ticket, `Landing` for a landing) and records who
/// holds it. Returns `false`, changing nothing, when the tree is missing or in any state but
/// `Free`: two sessions can never hold one tree, and a tree being reset or prebuilt is not
/// available yet.
pub fn lease(
    conn: &Connection,
    id: &str,
    to: State,
    ticket: Option<&str>,
    session: Option<&str>,
    branch: Option<&str>,
    now: &str,
) -> Result<bool> {
    if to == State::Free {
        return Ok(false);
    }
    let changed = conn.execute(
        "UPDATE trees SET state = ?2, ticket = ?3, session = ?4, branch = ?5, leased_at = ?6, \
         last_used = ?6 WHERE id = ?1 AND state = 'free'",
        params![id, to.as_str(), ticket, session, branch, now],
    )?;
    Ok(changed == 1)
}

/// Names the ticket a leased tree holds, keeping its branch. An ad hoc ticket's tree is leased
/// under a placeholder before the hub numbers the ticket, and is renamed here once it has.
/// Returns whether a leased tree was changed.
pub fn set_ticket(conn: &Connection, id: &str, ticket: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE trees SET ticket = ?2 WHERE id = ?1 AND state = 'leased'",
        params![id, ticket],
    )?;
    Ok(changed == 1)
}

/// Records the handle of the verb that holds a landing tree, so that a restarted daemon that
/// runs the verb again keeps the tree for it. Returns whether a landing tree was changed.
pub fn set_owner(conn: &Connection, id: &str, owner: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE trees SET owner = ?2 WHERE id = ?1 AND state = 'landing'",
        params![id, owner],
    )?;
    Ok(changed == 1)
}

/// Moves a tree from `from` to `to` only if it is in `from` right now, as one statement. Returns
/// `false`, changing nothing, when the tree is missing or in another state. Two callers that
/// both try to take a tree out of `from` therefore cannot both succeed.
pub fn transition(conn: &Connection, id: &str, from: State, to: State, now: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE trees SET state = ?3, last_used = ?4 WHERE id = ?1 AND state = ?2",
        params![id, from.as_str(), to.as_str(), now],
    )?;
    Ok(changed == 1)
}

/// Moves a tree from `Resetting` back to `Leased` only if its row still carries the same lease
/// (`ticket`, `session`, and `leased_at`) that a failed release claimed. Returns `false`,
/// changing nothing, when the tree is no longer resetting, when another operation replaced the
/// row (such as a sweep taking a free tree), or when neither a ticket nor a session is named.
pub fn restore_lease(
    conn: &Connection,
    id: &str,
    ticket: Option<&str>,
    session: Option<&str>,
    leased_at: Option<&str>,
    now: &str,
) -> Result<bool> {
    if ticket.is_none() && session.is_none() {
        return Ok(false);
    }
    let changed = conn.execute(
        "UPDATE trees SET state = 'leased', last_used = ?5 \
         WHERE id = ?1 AND state = 'resetting' \
           AND ticket IS ?2 AND session IS ?3 AND leased_at IS ?4",
        params![id, ticket, session, leased_at, now],
    )?;
    Ok(changed == 1)
}

/// Sets the state of an existing tree whatever state it is in. Returns whether the tree exists.
/// Use [`lease`] to take a free tree and [`transition`] to leave a state that others may also
/// be leaving; this does not check the old state.
pub fn set_state(conn: &Connection, id: &str, state: State, now: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE trees SET state = ?2, last_used = ?3 WHERE id = ?1",
        params![id, state.as_str(), now],
    )?;
    Ok(changed == 1)
}

/// Makes the tree `Free` again with no ticket, session, branch or owner, and records the commit
/// its build directories now match.
pub fn release(conn: &Connection, id: &str, built_commit: Option<&str>, now: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE trees SET state = 'free', ticket = NULL, session = NULL, branch = NULL, \
         leased_at = NULL, owner = NULL, built_commit = COALESCE(?2, built_commit), last_used = ?3 \
         WHERE id = ?1",
        params![id, built_commit, now],
    )?;
    Ok(changed == 1)
}

/// Deletes a tree's row if the tree is in `state` right now, as one statement. Returns whether a
/// row was deleted. Retirement removes the row after removing the tree from disk.
pub fn delete(conn: &Connection, id: &str, state: State) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM trees WHERE id = ?1 AND state = ?2",
        params![id, state.as_str()],
    )?;
    Ok(changed == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::{open, open_in_memory};

    fn sample(id: &str) -> rows::Tree {
        rows::Tree {
            id: id.to_owned(),
            path: format!("/trees/{id}"),
            kind: "ticket".to_owned(),
            state: "free".to_owned(),
            ticket: None,
            session: None,
            branch: None,
            built_commit: Some("8c1f2ab".to_owned()),
            leased_at: None,
            last_used: None,
            owner: None,
        }
    }

    /// An ad hoc ticket's tree, leased under a placeholder, takes the hub's number and keeps
    /// its branch; a free tree is never given a ticket this way.
    #[test]
    fn a_leased_tree_takes_its_tickets_number() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &sample("tree-01")).unwrap();
        upsert(&conn, &sample("tree-02")).unwrap();
        assert!(
            lease(
                &conn,
                "tree-01",
                State::Leased,
                Some("adhoc20261118141021392"),
                Some("local_a"),
                Some("adhoc20261118141021392"),
                "2026-11-18T00:00:00.000Z",
            )
            .unwrap()
        );
        assert!(set_ticket(&conn, "tree-01", "svn0").unwrap());
        let tree = get(&conn, "tree-01").unwrap().unwrap();
        assert_eq!(tree.ticket.as_deref(), Some("svn0"));
        assert_eq!(tree.branch.as_deref(), Some("adhoc20261118141021392"));
        assert!(!set_ticket(&conn, "tree-02", "svn0").unwrap());
        assert_eq!(get(&conn, "tree-02").unwrap().unwrap().ticket, None);
    }

    #[test]
    fn a_leased_tree_is_refused_until_released() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &sample("tree-01")).unwrap();
        let lease_it = |ticket| {
            lease(
                &conn,
                "tree-01",
                State::Leased,
                Some(ticket),
                Some("local_a"),
                Some("work/x"),
                "2026-11-18T00:00:00.000Z",
            )
            .unwrap()
        };
        assert!(lease_it("T.1"));
        assert!(!lease_it("T.2"));
        assert_eq!(
            get(&conn, "tree-01").unwrap().unwrap().ticket.as_deref(),
            Some("T.1")
        );
        assert!(
            release(
                &conn,
                "tree-01",
                Some("9d0e1f2"),
                "2026-11-18T01:00:00.000Z"
            )
            .unwrap()
        );
        let freed = get(&conn, "tree-01").unwrap().unwrap();
        assert_eq!(
            (
                freed.state.as_str(),
                freed.ticket,
                freed.built_commit.as_deref()
            ),
            ("free", None, Some("9d0e1f2"))
        );
        assert!(lease_it("T.2"));
    }

    /// Only a landing tree takes an owner, and freeing it forgets the owner.
    #[test]
    fn a_landing_tree_keeps_its_owner_until_released() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &sample("landing-1")).unwrap();
        assert!(!set_owner(&conn, "landing-1", "L-6").unwrap());
        assert!(
            lease(
                &conn,
                "landing-1",
                State::Landing,
                Some("m:T.1"),
                None,
                None,
                "t"
            )
            .unwrap()
        );
        assert!(set_owner(&conn, "landing-1", "L-6").unwrap());
        let held = get(&conn, "landing-1").unwrap().unwrap();
        assert_eq!(held.owner.as_deref(), Some("L-6"));
        assert_eq!(held.ticket.as_deref(), Some("m:T.1"));
        assert!(release(&conn, "landing-1", None, "t").unwrap());
        assert_eq!(get(&conn, "landing-1").unwrap().unwrap().owner, None);
    }

    #[test]
    fn a_transition_happens_only_from_the_expected_state() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &sample("tree-01")).unwrap();
        set_state(&conn, "tree-01", State::Leased, "t").unwrap();
        assert!(transition(&conn, "tree-01", State::Leased, State::Resetting, "t").unwrap());
        assert!(!transition(&conn, "tree-01", State::Leased, State::Resetting, "t").unwrap());
        assert!(!transition(&conn, "missing", State::Leased, State::Resetting, "t").unwrap());
        assert_eq!(get(&conn, "tree-01").unwrap().unwrap().state, "resetting");
    }

    #[test]
    fn restore_lease_requires_the_same_holder_and_resetting_state() {
        let conn = open_in_memory().unwrap();
        upsert(&conn, &sample("tree-01")).unwrap();
        assert!(
            lease(
                &conn,
                "tree-01",
                State::Leased,
                Some("T.1"),
                Some("sess-1"),
                Some("work/T.1"),
                "t1",
            )
            .unwrap()
        );
        assert!(transition(&conn, "tree-01", State::Leased, State::Resetting, "t2").unwrap());
        assert!(
            !restore_lease(&conn, "tree-01", Some("T.2"), Some("sess-1"), Some("t1"), "t3")
                .unwrap()
        );
        assert!(
            !restore_lease(&conn, "tree-01", Some("T.1"), Some("sess-2"), Some("t1"), "t3")
                .unwrap()
        );
        assert!(
            !restore_lease(&conn, "tree-01", Some("T.1"), Some("sess-1"), Some("t9"), "t3")
                .unwrap()
        );
        assert!(
            restore_lease(&conn, "tree-01", Some("T.1"), Some("sess-1"), Some("t1"), "t3")
                .unwrap()
        );
        assert_eq!(get(&conn, "tree-01").unwrap().unwrap().state, "leased");

        // A swept row in `resetting` with no ticket or session is never turned into `leased`.
        assert!(release(&conn, "tree-01", None, "t4").unwrap());
        assert!(lease(&conn, "tree-01", State::Resetting, None, None, None, "t5").unwrap());
        assert!(
            !restore_lease(&conn, "tree-01", Some("T.1"), Some("sess-1"), Some("t1"), "t6")
                .unwrap()
        );
        assert!(!restore_lease(&conn, "tree-01", None, None, Some("t5"), "t6").unwrap());
        assert_eq!(get(&conn, "tree-01").unwrap().unwrap().state, "resetting");
    }

    #[test]
    fn only_a_free_tree_can_be_leased() {
        let conn = open_in_memory().unwrap();
        for state in [
            State::Prebuilding,
            State::Landing,
            State::Resetting,
            State::Retiring,
        ] {
            upsert(&conn, &sample("tree-01")).unwrap();
            set_state(&conn, "tree-01", state, "t").unwrap();
            assert!(!lease(&conn, "tree-01", State::Leased, None, None, None, "t").unwrap());
        }
        assert!(!lease(&conn, "missing", State::Leased, None, None, None, "t").unwrap());
        assert!(!lease(&conn, "tree-01", State::Free, None, None, None, "t").unwrap());
    }

    #[test]
    fn trees_survive_reopening_the_database() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-trees-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("local.db");
        let _ = std::fs::remove_file(&path);
        let mut tree = sample("tree-02");
        tree.state = "leased".to_owned();
        tree.ticket = Some("T.1".to_owned());
        {
            let conn = open(&path).unwrap();
            upsert(&conn, &tree).unwrap();
            upsert(&conn, &sample("tree-01")).unwrap();
        }
        let conn = open(&path).unwrap();
        let trees = all(&conn).unwrap();
        assert_eq!(trees, vec![sample("tree-01"), tree]);
        // Windows deletes no file that a connection still holds open.
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
