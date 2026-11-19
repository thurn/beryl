//! Whether a landing gates in its ticket tree or in a landing tree.

use super::*;

/// tree-01 holds a build at the tip, the change's rebased commit; landing-1, the only landing
/// tree, holds one a commit behind. Every commit edits `a.txt`, which lies beside `out`.
fn ticket_built_at_the_tip() -> Fixture {
    let f = Fixture::new(1, 3);
    write(&f.repo.join(".worktrees/tree-01"), "out/built", "x");
    write(&f.repo.join(".worktrees/landing-1"), "out/built", "x");
    f.trees
        .record_build(&id("landing-1"), &f.commits[1])
        .unwrap();
    f
}

#[test]
fn the_ticket_tree_builds_less_when_its_build_is_nearer_the_change() {
    let f = ticket_built_at_the_tip();
    let change = ["a.txt".to_owned()];
    assert!(
        f.trees
            .ticket_builds_less(&id("tree-01"), &f.tip(), &f.tip(), &change)
            .unwrap()
    );
    // At the same distance the landing tree is kept, so that it moves nearer the tip.
    assert!(
        !f.trees
            .ticket_builds_less(&id("tree-01"), &f.commits[1], &f.tip(), &change)
            .unwrap()
    );
}

#[test]
fn a_ticket_tree_without_the_build_directory_the_change_wants_builds_more() {
    let f = ticket_built_at_the_tip();
    fs::remove_dir_all(f.repo.join(".worktrees/tree-01/out")).unwrap();
    let change = ["a.txt".to_owned()];
    assert!(
        !f.trees
            .ticket_builds_less(&id("tree-01"), &f.tip(), &f.tip(), &change)
            .unwrap()
    );
}

#[test]
fn with_no_free_landing_tree_the_ticket_tree_is_not_preferred() {
    let f = ticket_built_at_the_tip();
    let _held = f
        .trees
        .lease_landing("tip verification", &f.tip())
        .unwrap()
        .unwrap();
    let change = ["a.txt".to_owned()];
    assert!(
        !f.trees
            .ticket_builds_less(&id("tree-01"), &f.tip(), &f.tip(), &change)
            .unwrap()
    );
}
