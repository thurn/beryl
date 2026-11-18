//! Trees that `ignite.toml` no longer declares are removed once free, and the disk's floor of
//! free space holds the trees under their current size when no limit would.

use super::*;

/// A row for `tree` in `trees`' store, as the daemon would have written it when `ignite.toml`
/// still declared the tree.
fn stored(trees: &Trees, f: &Fixture, tree: &str, state: store::State, session: Option<&str>) {
    let row = rows::Tree {
        id: tree.to_owned(),
        path: f.repo.join(".worktrees").join(tree).display().to_string(),
        kind: Kind::Ticket.as_str().to_owned(),
        state: state.as_str().to_owned(),
        ticket: session.map(|_| "T.9".to_owned()),
        session: session.map(str::to_owned),
        branch: None,
        built_commit: None,
        leased_at: None,
        last_used: None,
        owner: None,
    };
    store::upsert(&trees.lock().db, &row).unwrap();
}

#[test]
fn a_free_undeclared_tree_is_removed_and_a_leased_one_kept_for_its_session() {
    let f = Fixture::new(3, 1);
    let worktrees = f.repo.join(".worktrees");
    for tree in ["tree-02", "tree-03"] {
        write(&worktrees.join(tree), "out/big", &"x".repeat(10_000));
    }
    // The machine now declares one ticket tree.
    let mut config = f.trees.config().clone();
    config.ticket_trees = 1;
    let fewer = Trees::new(
        config,
        open_in_memory().unwrap(),
        Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        )),
        f.builds.clone(),
    );
    fewer.setup(&f.tip()).unwrap();
    stored(&fewer, &f, "tree-02", store::State::Free, None);
    stored(&fewer, &f, "tree-03", store::State::Leased, Some("local_a"));

    // The session that holds tree-03 is still in a tree, so it can go on and release it.
    assert_eq!(
        fewer.tree_at(&worktrees.join("tree-03/out")),
        Some(id("tree-03"))
    );

    assert_eq!(fewer.retire_undeclared().unwrap(), [id("tree-02")]);
    assert!(!worktrees.join("tree-02").exists());
    assert!(worktrees.join("tree-03/out/big").exists());
    let ids: Vec<String> = store::all(&fewer.lock().db)
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert!(!ids.contains(&"tree-02".to_owned()), "{ids:?}");
    assert!(ids.contains(&"tree-03".to_owned()), "{ids:?}");
    assert_eq!(f.builds.forgotten(), [id("tree-02")]);
    // Nothing else is removed: tree-01 is declared, and tree-03 is still leased.
    assert!(fewer.retire_undeclared().unwrap().is_empty());
    assert!(worktrees.join("tree-01/.git").exists());
}

#[test]
fn a_disk_short_of_free_space_sweeps_free_trees_with_no_limit_set() {
    let f = Fixture::new(2, 1);
    for tree in ["tree-01", "tree-02"] {
        write(
            &f.repo.join(".worktrees").join(tree),
            "out/big",
            &"x".repeat(200_000),
        );
    }
    let report = f.trees.hold_disk(0).unwrap();
    assert!(report.cleared.is_empty(), "{report:?}");
    assert_eq!(report.limit, None);

    let report = f.trees.hold_disk(1).unwrap();
    assert_eq!(report.limit, Some(report.before - 1));
    assert_eq!(report.cleared.len(), 1, "{report:?}");
    assert!(report.after < report.before);
}
