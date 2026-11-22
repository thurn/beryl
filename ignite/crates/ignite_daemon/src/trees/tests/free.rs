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
        holds_tip: false,
        epic: None,
        priority: 0,
        chain: 0,
        created_at: Timestamp::EPOCH,
        labels: vec![],
        expected_cores: 0.1,
        families: vec![],
        files: vec![],
        saving: None,
        pins: ignite_core::pins::Pins::default(),
        needs_os: None,
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

    let report = f
        .trees
        .reclaim_disk_for_handoff_with(free_gb, &|| false)
        .unwrap();

    assert_eq!(report.cleared, free_trees[..2]);
    assert!(f.trees.config().path(&free_trees[2]).join("out").exists());
    assert!(first.path.join("out").exists());
    assert!(second.path.join("out").exists());
    assert!(blocked().is_none());
}

/// A measurement of the trees walked 290 GB for over ten minutes, and the disk can fall below
/// its floor during one. Asked to stop partway through the walk, the sweep returns at once,
/// marked as interrupted, without clearing anything on the strength of a partial count.
#[test]
fn a_measurement_asked_to_stop_partway_returns_at_once_and_clears_nothing() {
    let f = Fixture::new(3, 1);
    for tree in ["tree-01", "tree-02", "tree-03"] {
        let root = f.repo.join(".worktrees").join(tree);
        for dir in ["a", "b", "c"] {
            write(&root, &format!("out/{dir}/deep/big"), &"x".repeat(10_000));
        }
    }
    // How often an uninterrupted sweep asks: the last questions come from the measurement.
    let asked = Cell::new(0);
    let report = f
        .trees
        .hold_disk_until(|_| None, &|| {
            asked.set(asked.get() + 1);
            false
        })
        .unwrap();
    assert!(!report.interrupted);
    let full = asked.get();
    assert!(
        full > 3,
        "the walk asks before each directory, not once: {full}"
    );

    asked.set(0);
    let report = f
        .trees
        .hold_disk_until(|_| Some((1, DiskLimitSource::Cap)), &|| {
            asked.set(asked.get() + 1);
            asked.get() >= full - 1
        })
        .unwrap();

    assert!(report.interrupted, "{report:?}");
    assert!(report.cleared.is_empty(), "{report:?}");
    assert_eq!(
        report.limit, None,
        "the budget is not read from a partial count"
    );
    for tree in ["tree-01", "tree-02", "tree-03"] {
        assert!(
            f.repo
                .join(".worktrees")
                .join(tree)
                .join("out/a/deep/big")
                .exists()
        );
    }
    // Never asked to stop, the same sweep clears trees to reach the limit.
    let report = sweep_to(&f.trees, Some(1));
    assert!(!report.interrupted);
    assert!(!report.cleared.is_empty(), "{report:?}");
}
