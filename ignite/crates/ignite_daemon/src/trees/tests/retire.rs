//! The disk loop retires old free trees and sweeps builds when free disk drops below its floor.

use super::*;

#[test]
fn retirement_keeps_the_newest_two_free_trees_and_skips_leases() {
    let f = Fixture::new(4, 1);
    let leases: Vec<_> = (1..=4).map(|n| f.lease(&format!("T.{n}"), false)).collect();
    write(&leases[0].path, "out/big", "discarded");
    for lease in &leases[..3] {
        f.trees
            .release_unsaved(&lease.tree, &f.tip(), None)
            .unwrap();
        f.clock.advance(1.0);
    }

    f.clock.advance(86_396.0);
    assert!(f.trees.retire_surplus().unwrap().is_empty());
    f.clock.advance(4.0);
    assert_eq!(f.trees.retire_surplus().unwrap(), [leases[0].tree.clone()]);
    assert!(!leases[0].path.exists());
    assert!(
        store::get(&f.trees.lock().db, leases[0].tree.as_str())
            .unwrap()
            .is_none()
    );
    assert_eq!(f.builds.forgotten(), [leases[0].tree.clone()]);
    for lease in &leases[1..] {
        assert!(lease.path.join(".git").is_file());
    }
    assert_eq!(f.trees.free_ticket_trees().unwrap(), 3);
    assert!(f.trees.retire_surplus().unwrap().is_empty());
}

#[test]
fn a_restart_does_not_recreate_a_tree_whose_retirement_removed_its_checkout() {
    let f = Fixture::new(1, 1);
    let tree = id("tree-01");
    let path = f.trees.config().path(&tree);
    store::set_state(
        &f.trees.lock().db,
        tree.as_str(),
        store::State::Retiring,
        &f.trees.now(),
    )
    .unwrap();
    worktree::remove(&f.repo, &path).unwrap();

    assert!(f.trees.mark_unfinished().unwrap().is_empty());
    assert_eq!(f.trees.retire_surplus().unwrap(), [tree]);
    assert!(!path.exists());
    assert_eq!(f.trees.free_ticket_trees().unwrap(), 1);
}
