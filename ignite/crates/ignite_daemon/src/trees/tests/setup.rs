//! A restarted daemon marks the trees it must create or reset before it serves its socket, and
//! creates or resets them afterwards; a lease that needs one of them waits for it.

use ignite_git::GitCli;

use super::*;

/// A ticket tree left `resetting` by a release the previous daemon was cut off in, with a file
/// a reset deletes.
fn left_resetting(f: &Fixture) -> (TreeLease, PathBuf) {
    let lease = f.lease("T.1", false);
    let marker = lease.path.join("half-released.txt");
    fs::write(&marker, "left by a release cut off by a restart\n").unwrap();
    store::set_state(
        &f.trees.lock().db,
        lease.tree.as_str(),
        store::State::Resetting,
        "t",
    )
    .unwrap();
    (lease, marker)
}

#[test]
fn marking_unfinished_trees_resets_nothing_and_a_lease_waits_for_the_reset() {
    let f = Fixture::new(1, 1);
    let (lease, marker) = left_resetting(&f);

    let unfinished = f.trees.mark_unfinished().unwrap();

    assert_eq!(
        unfinished,
        [Unfinished {
            tree: lease.tree.clone(),
            fresh: false,
        }]
    );
    assert_eq!(f.state("tree-01"), "resetting");
    assert_eq!(f.trees.status().unwrap().trees[0].ticket, None);
    assert!(marker.exists(), "marking ran a reset");
    let Lease::Waiting(waiter) = f.trees.lease(f.request("T.2", false)).unwrap() else {
        panic!("a tree that is being reset was leased");
    };
    assert!(waiter.wait(Duration::ZERO).is_none());

    f.trees.finish(&unfinished[0], &f.tip()).unwrap();

    assert!(!marker.exists(), "the reset removes untracked files");
    let served = waiter.wait(Duration::ZERO).unwrap().unwrap();
    assert_eq!(served.tree, lease.tree);
    assert_eq!(f.state("tree-01"), "leased");
}

#[test]
fn a_landing_tree_left_landing_is_marked_and_freed_by_the_background_reset() {
    let f = Fixture::new(1, 1);
    let landing = f
        .trees
        .lease_landing("machine:T.1", &f.tip())
        .unwrap()
        .unwrap();
    let marker = landing.path.join("gate-output.txt");
    fs::write(&marker, "half done\n").unwrap();

    // The daemon restarts: the landing has no owner that runs again.
    let unfinished = f.trees.mark_unfinished().unwrap();
    assert_eq!(f.state(landing.tree.as_str()), "resetting");
    assert!(
        f.trees
            .lease_landing("machine:T.2", &f.tip())
            .unwrap()
            .is_none(),
        "a landing tree that is being reset was leased"
    );
    // A ticket lease waits behind the reset too, and is served by the same thread.
    let (ticket_lease, _) = left_resetting(&f);
    let unfinished: Vec<_> = unfinished
        .into_iter()
        .chain(f.trees.mark_unfinished().unwrap())
        .collect();
    let Lease::Waiting(waiter) = f.trees.lease(f.request("T.3", false)).unwrap() else {
        panic!("a tree that is being reset was leased");
    };

    let git: Arc<dyn ignite_core::traits::Git> = Arc::new(GitCli {
        repo: f.repo.clone(),
        remote: "origin".to_owned(),
    });
    f.trees
        .finish_in_background(git, "main".to_owned(), unfinished);

    let served = waiter.wait(Duration::from_secs(120)).unwrap().unwrap();
    assert_eq!(served.tree, ticket_lease.tree);
    assert!(!marker.exists(), "the landing tree was reset");
    assert_eq!(f.state(landing.tree.as_str()), "free");
}

#[test]
fn a_missing_tree_is_marked_fresh_and_created_by_finishing() {
    let f = Fixture::new(1, 1);
    let path = f.trees.config().path(&id("tree-01"));
    fs::remove_dir_all(&path).unwrap();
    git(&f.repo, &["worktree", "prune"]);

    let unfinished = f.trees.mark_unfinished().unwrap();

    assert_eq!(
        unfinished,
        [Unfinished {
            tree: id("tree-01"),
            fresh: true,
        }]
    );
    assert_eq!(f.state("tree-01"), "resetting");
    f.trees.finish(&unfinished[0], &f.tip()).unwrap();
    assert!(path.join(".git").exists());
    assert_eq!(f.state("tree-01"), "free");
    f.lease("T.1", false);
}
