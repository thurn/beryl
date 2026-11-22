//! Elastic leases and the persisted state used to recover unfinished worktrees.

use std::sync::Barrier;

use super::*;

#[test]
fn a_configuration_without_ticket_trees_creates_one_for_each_lease() {
    let f = Fixture::new(0, 1);
    let config = Config::parse(
        "ignite.toml",
        r#"
        [repository]
        remote = "origin"
        staging = "main"
        master = "master"
        trees = ".worktrees"
        checks = "checks"
        [hub]
        url = "http://localhost:8080"
        project = "p"
        [gate]
        threshold = 1.0
        cap = 10
        "#,
    )
    .unwrap();
    let mut config = TreeConfig::new(
        &f.repo,
        &config,
        &Derived {
            express_cores: 2,
            tip_build_cores: 2,
            floor: ignite_core::capacity::Floor::default(),
            landing_trees: 1,
            worker_ceiling: 2,
        },
        &Manifest::default(),
    );
    assert_eq!(config.ticket_tree_ceiling, 6);
    config.ticket_tree_ceiling = 2;
    let trees = Trees::new(
        config,
        open_in_memory().unwrap(),
        f.clock.clone(),
        f.builds.clone(),
    );
    trees.setup(&f.tip()).unwrap();
    assert_eq!(trees.free_ticket_trees().unwrap(), 2);
    for (ticket, expected) in [("T.1", "tree-01"), ("T.2", "tree-02")] {
        let Lease::Ready(lease) = trees.lease(f.request(ticket, false)).unwrap() else {
            panic!("a lease should create a ticket tree");
        };
        assert_eq!(lease.tree.as_str(), expected);
        assert_eq!(
            git(&lease.path, &["branch", "--show-current"]).trim(),
            format!("work/{ticket}")
        );
    }
    assert_eq!(trees.free_ticket_trees().unwrap(), 0);
    let Lease::Waiting(waiter) = trees.lease(f.request("T.3", false)).unwrap() else {
        panic!("the tree ceiling should stop a new checkout");
    };
    assert!(waiter.reason().contains("all 2 ticket trees are in use"));
    assert!(!trees.config().dir.join("tree-03").exists());
}

#[test]
fn simultaneous_leases_allocate_distinct_tree_names() {
    let mut f = Fixture::new(0, 1);
    let mut config = f.trees.config().clone();
    config.ticket_tree_ceiling = 2;
    f.trees = Trees::new(
        config,
        open_in_memory().unwrap(),
        f.clock.clone(),
        f.builds.clone(),
    );
    let barrier = Barrier::new(2);
    let mut ids = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            f.lease("T.1", false).tree
        });
        let b = scope.spawn(|| {
            barrier.wait();
            f.lease("T.2", false).tree
        });
        [a.join().unwrap(), b.join().unwrap()]
    });
    ids.sort();
    assert_eq!(ids, [id("tree-01"), id("tree-02")]);
}

#[test]
fn startup_recovers_a_parked_worktree_without_a_row() {
    let mut f = Fixture::new(0, 1);
    let mut config = f.trees.config().clone();
    config.ticket_tree_ceiling = 1;
    let tree = id("tree-01");
    let path = config.path(&tree);
    worktree::add(&f.repo, &path, &park_branch(&tree), &f.tip()).unwrap();

    f.trees = Trees::new(
        config,
        open_in_memory().unwrap(),
        f.clock.clone(),
        f.builds.clone(),
    );
    let pending = f.trees.mark_unfinished().unwrap();
    let unfinished = pending.iter().find(|item| item.tree == tree).unwrap();
    assert!(!unfinished.fresh);
    f.trees.finish(unfinished, &f.tip()).unwrap();
    let lease = f.lease("T.1", false);
    assert_eq!(lease.tree, tree);
    assert_eq!(lease.path, path);
}

#[test]
fn startup_finishes_an_allocation_reserved_before_checkout() {
    let f = Fixture::new(0, 1);
    let tree = id("tree-01");
    let row =
        crate::trees::setup::resetting_tree(&f.trees.config().path(&tree), &tree, Kind::Ticket);
    store::upsert(&f.trees.lock().db, &row).unwrap();
    let pending = f.trees.mark_unfinished().unwrap();
    assert_eq!(
        pending,
        [Unfinished {
            tree: tree.clone(),
            fresh: true
        }]
    );
    f.trees.finish(&pending[0], &f.tip()).unwrap();
    assert_eq!(f.lease("T.1", false).tree, tree);
}
