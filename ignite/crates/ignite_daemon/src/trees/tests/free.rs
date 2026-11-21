//! The disk loop clears free trees one at a time until there is enough free space.

use std::cell::Cell;

use ignite_core::dispatch::{ReadyTicket, TicketKind};
use ignite_core::dispatch_pressure::{Budget, DispatchPressure};

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

#[test]
fn a_handoff_waiting_for_disk_sweeps_free_trees_while_builds_are_held() {
    let f = Fixture::new(5, 1);
    let first = f.lease("T.1", false);
    let second = f.lease("T.2", false);
    let worktrees = f.repo.join(".worktrees");
    let mut free_trees = Vec::new();
    for name in ["tree-01", "tree-02", "tree-03", "tree-04", "tree-05"] {
        write(&worktrees.join(name), &format!("out/{name}"), "build");
        if f.state(name) == "free" {
            free_trees.push(id(name));
        }
    }
    let free_gb = || {
        let cleared = free_trees
            .iter()
            .filter(|id| !f.trees.config().path(id).join("out").exists())
            .count() as u64;
        Some(65 + 35 * cleared)
    };
    let waiting = ReadyTicket {
        id: TicketId::new("T.3").unwrap(),
        kind: TicketKind::Ordinary,
        epic: None,
        priority: 0,
        chain: 0,
        created_at: Timestamp::EPOCH,
        labels: vec![],
        expected_cores: 0.1,
        families: vec![],
        files: vec![],
        saving: None,
        destinations: vec![],
    };
    let held = [TicketId::new("T.1").unwrap(), TicketId::new("T.2").unwrap()];
    let blocked = || {
        let pressure = DispatchPressure {
            free_gb: free_gb(),
            ..DispatchPressure::default()
        };
        let mut budget = Budget::new(&pressure, Timestamp::EPOCH);
        for ticket in &held {
            budget.hold(ticket, 1.0, &[], None);
        }
        budget.blocked(&waiting)
    };
    let wait = blocked().expect("the held builds leave too little room for the handoff");
    let target = wait.reclaim_to_gb.expect("a disk refusal requests a sweep");
    assert_eq!(target, 131);
    f.trees.request_disk_for_handoff(target);

    let report = f.trees.reclaim_disk_for_handoff_with(free_gb).unwrap();

    assert_eq!(report.cleared, free_trees[..2]);
    assert!(f.trees.config().path(&free_trees[2]).join("out").exists());
    assert!(first.path.join("out").exists());
    assert!(second.path.join("out").exists());
    assert!(blocked().is_none());
}
