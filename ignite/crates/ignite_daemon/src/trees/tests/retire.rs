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

#[test]
fn a_root_change_retires_free_trees_and_lets_existing_leases_finish() {
    let f = Fixture::new(2, 1);
    let held = f.lease("T.1", false);
    let old_free = f.trees.config().path(&id("tree-02"));
    let old_landing = f.trees.config().path(&id("landing-1"));
    let new_root = f.scratch.0.join("new-trees");
    let notice = ignite_core::fleet::FleetNotice {
        trees: ignite_core::fleet::TreeSettings {
            trees_root: Some(new_root.display().to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    crate::fleet_notice::write(&f.trees.lock().db, &notice).unwrap();

    assert_eq!(
        f.trees.retire_surplus().unwrap(),
        [id("landing-1"), id("tree-02")]
    );
    assert!(!old_free.exists());
    assert!(!old_landing.exists());
    assert_eq!(f.trees.tree_at(&held.path), Some(held.tree.clone()));
    assert!(held.path.join(".git").is_file());

    let new_ticket = f.lease("T.2", false);
    assert_eq!(new_ticket.path, new_root.join(new_ticket.tree.as_str()));
    assert!(new_ticket.path.join(".git").is_file());
    let pending = f.trees.mark_missing_landings().unwrap();
    assert_eq!(pending.len(), 1);
    f.trees.finish(&pending[0], &f.tip()).unwrap();
    let landing = f.trees.lease_landing("gate", &f.tip()).unwrap().unwrap();
    assert_eq!(landing.path, new_root.join("landing-1"));

    f.trees.release_unsaved(&held.tree, &f.tip(), None).unwrap();
    assert_eq!(f.trees.retire_surplus().unwrap(), [held.tree]);
    assert!(!held.path.exists());
}
