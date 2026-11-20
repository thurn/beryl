//! Below the disk's floor of free space, free trees are cleared one at a time, without a
//! measurement, until the disk is back above it.

use std::cell::Cell;

use super::*;

/// The disk filled to 4 GB free while the disk loop spent each round measuring 1.6 TB of build
/// directories. Clearing reads only the disk's free space, after each tree, and stops as soon
/// as it is back above the floor; a leased tree is never touched.
#[test]
fn free_trees_are_cleared_until_the_disk_is_back_above_its_floor() {
    let f = Fixture::new(3, 1);
    for tree in ["tree-01", "tree-02", "tree-03"] {
        write(
            &f.repo.join(".worktrees").join(tree),
            "out/big",
            &"x".repeat(10_000),
        );
    }
    let leased = match f.trees.lease(f.request("T.1", false)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };
    // The disk is short of the floor until one tree is cleared.
    let reads = Cell::new(0);
    let report = f
        .trees
        .free_disk(100, || {
            reads.set(reads.get() + 1);
            Some(if reads.get() == 1 { 10 } else { 200 })
        })
        .unwrap();

    assert_eq!(report.cleared.len(), 1, "{report:?}");
    assert_eq!(reads.get(), 2);
    let cleared = &report.cleared[0];
    assert_ne!(cleared, &leased.tree);
    assert!(!f.trees.config().path(cleared).join("out").exists());
    assert!(leased.path.join("out/big").exists());
    assert_eq!(f.builds.forgotten(), report.cleared);

    // Free space that cannot be read clears nothing.
    let report = f.trees.free_disk(100, || None).unwrap();
    assert!(report.cleared.is_empty());
}
