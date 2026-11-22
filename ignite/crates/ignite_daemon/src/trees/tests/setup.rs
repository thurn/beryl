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

#[test]
fn after_restart_a_landing_keeps_its_old_path_until_retired() {
    let f = Fixture::new(1, 1);
    let landing = f.trees.lease_landing("gate", &f.tip()).unwrap().unwrap();
    let new_root = f.scratch.0.join("new-trees");
    crate::fleet_notice::write(
        &f.trees.lock().db,
        &ignite_core::fleet::FleetNotice {
            trees: ignite_core::fleet::TreeSettings {
                trees_root: Some(new_root.display().to_string()),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();

    let pending = f.trees.mark_unfinished().unwrap();
    assert_eq!(
        pending,
        [Unfinished {
            tree: landing.tree.clone(),
            fresh: false
        }]
    );
    f.trees.finish(&pending[0], &f.tip()).unwrap();
    assert_eq!(f.trees.path(&landing.tree), Some(landing.path.clone()));
    assert_eq!(
        f.trees.retire_surplus().unwrap(),
        [landing.tree, id("tree-01")]
    );
    assert!(!landing.path.exists());
    let pending = f.trees.mark_missing_landings().unwrap();
    f.trees.finish(&pending[0], &f.tip()).unwrap();
    assert!(new_root.join("landing-1/.git").is_file());
}

#[test]
fn doctor_reports_cargo_targets_only_in_registered_worktrees_outside_the_tree_root_and_ignite() {
    use crate::doctor::{GitTrees, TreeSource};

    let f = Fixture::new(1, 1);
    let stray = f.scratch.0.join("outside");
    worktree::add(&f.repo, &stray, "outside", &f.tip()).unwrap();
    write(&stray, "other/target/.rustc_info.json", "{}");
    write(&stray, "empty/target/readme", "not a build");
    write(&f.repo, "target/.rustc_info.json", "{}");
    write(
        &f.trees.config().path(&id("tree-01")),
        "target/.rustc_info.json",
        "{}",
    );
    write(&f.scratch.0, "unregistered/target/.rustc_info.json", "{}");
    // The daemon keeps its update tree's build between releases.
    let update = f.scratch.0.join("update-tree");
    worktree::add(&f.repo, &update, "ignite-update", &f.tip()).unwrap();
    write(&update, "ignite/target/.rustc_info.json", "{}");
    let source = GitTrees {
        repo: f.repo.clone(),
        dir: f.trees.root(),
        own: vec![update],
    };
    assert_eq!(source.stray_builds().unwrap(), [stray.join("other/target")]);
}

#[test]
fn a_new_root_cannot_replace_an_unrelated_landing_directory() {
    let f = Fixture::new(0, 1);
    let root = f.scratch.0.join("new-trees");
    let marker = root.join("landing-1/keep-me");
    write(&root, "landing-1/keep-me", "unrelated contents");
    crate::fleet_notice::write(
        &f.trees.lock().db,
        &ignite_core::fleet::FleetNotice {
            trees: ignite_core::fleet::TreeSettings {
                trees_root: Some(root.display().to_string()),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    f.trees.retire_surplus().unwrap();
    assert!(f.trees.mark_missing_landings().is_err());
    assert!(f.trees.path(&id("landing-1")).is_none());
    assert_eq!(fs::read_to_string(marker).unwrap(), "unrelated contents");
}
