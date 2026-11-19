use super::*;

fn seed(conn: &Connection, rows: &[(&str, Option<&str>, u32)]) {
    for (id, parent, next_child) in rows {
        conn.execute(
            "INSERT INTO tickets (id, parent, next_child, title, created_at, created_by, \
             updated_at, version) VALUES (?1, ?2, ?3, ?1, 'x', 'x', 'x', 1)",
            params![id, parent, next_child],
        )
        .unwrap();
    }
}

fn ids(conn: &Connection) -> Vec<(String, Option<String>)> {
    let mut statement = conn
        .prepare("SELECT id, parent FROM tickets ORDER BY id")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[test]
fn the_migration_renumbers_moved_tickets_under_their_parents_and_keeps_their_old_ids() {
    let mut conn = crate::hub::open_in_memory().unwrap();
    seed(
        &conn,
        &[
            // A root ticket moved under an epic, with a child of its own.
            ("EME7A", None, 2),
            ("EME7A.1", Some("EME7A"), 1),
            ("TQ44S", None, 114),
            ("TQ44S.113", Some("EME7A"), 2),
            ("TQ44S.113.1", Some("TQ44S.113"), 1),
            // A child whose parent was taken away.
            ("T7PM", None, 3),
            ("T7PM.2.4", None, 1),
            // A child whose parent no longer exists.
            ("TGONE.1", Some("TGONE"), 1),
        ],
    );
    conn.execute(
        "INSERT INTO edges (src, dst, kind) VALUES ('EME7A.1', 'TQ44S.113', 'blocks')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO comments (ticket, n, author, at, text) VALUES ('TQ44S.113', 1, 'a', 'x', 'c')",
        [],
    )
    .unwrap();
    let before = feed::current(&conn).unwrap();
    let transaction = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    renumber_all(&transaction).unwrap();
    transaction.commit().unwrap();

    assert!(violations(&conn).unwrap().is_empty(), "{:?}", ids(&conn));
    let resolve = |name: &str| crate::hub::tickets::resolve(&conn, name).unwrap().unwrap();
    assert_eq!(resolve("TQ44S.113").as_str(), "EME7A.2");
    assert_eq!(resolve("TQ44S.113.1").as_str(), "EME7A.2.1");
    let detached = resolve("T7PM.2.4");
    assert!(detached.as_str().starts_with('T') && !detached.as_str().contains('.'));
    let orphan = resolve("TGONE.1");
    assert!(!orphan.as_str().contains('.'), "{orphan}");
    let next: u32 = conn
        .query_row(
            "SELECT next_child FROM tickets WHERE id = 'EME7A'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(next, 3);
    let dst: String = conn
        .query_row("SELECT dst FROM edges WHERE src = 'EME7A.1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(dst, "EME7A.2");
    let commented: String = conn
        .query_row("SELECT ticket FROM comments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(commented, "EME7A.2");
    // Each renamed ticket is announced at one new feed position, for the mirrors.
    let announced: i64 = conn
        .query_row(
            "SELECT count(*) FROM feed WHERE seq = ?1 AND kind = 'ticket'",
            [before.0 as i64 + 1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(announced, 4);
}

#[test]
fn the_migration_leaves_a_tree_that_follows_the_rule_alone() {
    let mut conn = crate::hub::open_in_memory().unwrap();
    seed(&conn, &[("EA", None, 2), ("EA.1", Some("EA"), 1)]);
    let before = feed::current(&conn).unwrap();
    let transaction = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    renumber_all(&transaction).unwrap();
    transaction.commit().unwrap();
    assert_eq!(
        ids(&conn),
        [
            ("EA".to_owned(), None),
            ("EA.1".to_owned(), Some("EA".to_owned()))
        ]
    );
    assert_eq!(feed::current(&conn).unwrap(), before);
}
