//! The disk loop sweeps leased trees that run nothing: their stale object files and replaced
//! builds, and, when the trees stay over their limit with no free tree left, the whole build
//! directories of the largest.
//!
//! Other tests in this crate register running verbs from `tree-01` with no path, which the
//! disk loop takes for a verb in this fixture's `tree-01` too, so these tests assert nothing
//! that depends on `tree-01` being idle.

use std::cell::Cell;

use super::*;

const MB: usize = 1_000_000;

/// Every ticket tree in the fixture, leased with `sizes[i]` bytes in its build file.
fn all_leased(f: &Fixture, sizes: &[usize]) -> Trees {
    for (i, size) in sizes.iter().enumerate() {
        let tree = format!("tree-0{}", i + 1);
        write(
            &f.repo.join(".worktrees").join(tree),
            "out/big",
            &"x".repeat(*size),
        );
    }
    let trees = Trees::new(
        f.trees.config().clone(),
        open_in_memory().unwrap(),
        Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        )),
        f.builds.clone(),
    );
    trees.setup(&f.tip()).unwrap();
    for n in 0..sizes.len() {
        let Lease::Ready(lease) = trees.lease(f.request(&format!("T.{n}"), true)).unwrap() else {
            panic!("expected a free tree");
        };
        // A compiling lease returns before the copy of another tree's build into it ends, and
        // the disk loop leaves a tree alone while it is seeded or seeds another.
        crate::trees::wait_until_seeded(&lease.path).unwrap();
    }
    trees
}

fn built_commit(trees: &Trees, tree: &str) -> Option<String> {
    trees
        .status()
        .unwrap()
        .trees
        .into_iter()
        .find(|t| t.tree.as_str() == tree)
        .unwrap()
        .built_commit
}

fn state(trees: &Trees, tree: &str) -> String {
    trees
        .status()
        .unwrap()
        .trees
        .into_iter()
        .find(|t| t.tree.as_str() == tree)
        .unwrap()
        .state
}

/// On jarvis every tree was leased and grew to 150 to 200 GB while the disk loop could clear
/// nothing. With no free tree, the idle leased tree holding the most bytes is cleared, and only
/// as many trees as bring the trees under the limit.
#[test]
fn with_every_tree_leased_the_largest_idle_one_is_cleared() {
    let f = Fixture::new(3, 1);
    let trees = all_leased(&f, &[MB, 3 * MB, 2 * MB]);
    let total = sweep_to(&trees, None).before;

    let report = sweep_to(&trees, Some(total - 5 * MB as u64 / 2));

    assert!(report.cleared.is_empty(), "{report:?}");
    assert_eq!(report.reclaimed, [id("tree-02")], "{report:?}");
    assert!(report.after <= report.limit.unwrap(), "{report:?}");
    let worktrees = f.repo.join(".worktrees");
    assert!(!worktrees.join("tree-02/out").exists());
    assert!(worktrees.join("tree-03/out/big").exists());
    assert!(f.builds.forgotten().contains(&id("tree-02")));
    // The tree stays leased, and no longer says it holds a build.
    assert_eq!(state(&trees, "tree-02"), "leased");
    assert_eq!(built_commit(&trees, "tree-02"), None);
}

/// A leased tree with a verb running from it is never swept.
#[test]
fn a_leased_tree_running_a_verb_is_left_alone() {
    let f = Fixture::new(3, 1);
    let trees = all_leased(&f, &[MB, 2 * MB, 3 * MB]);
    let busy = crate::steps::global().begin(
        crate::steps::Kind::Verb("check".to_owned()),
        crate::steps::Work {
            tree: Some(id("tree-03")),
            tree_path: Some(trees.config().path(&id("tree-03"))),
            ..crate::steps::Work::default()
        },
    );

    let report = sweep_to(&trees, Some(1));
    drop(busy);

    assert!(report.reclaimed.contains(&id("tree-02")), "{report:?}");
    assert!(!report.reclaimed.contains(&id("tree-03")), "{report:?}");
    assert!(f.repo.join(".worktrees/tree-03/out/big").exists());
}

/// Stale object files are deleted from leased trees in every round, whether or not the trees
/// are over their limit.
#[test]
fn stale_object_files_in_a_leased_tree_are_deleted_under_the_limit_too() {
    let f = Fixture::new(2, 1);
    let trees = all_leased(&f, &[0, 0]);
    let profile = f.repo.join(".worktrees/tree-02/out/debug");
    write(&profile, ".fingerprint/app-0a0a/bin-app", "");
    let old = profile.join("deps/app-0a0a.cgu.first.rcgu.o");
    write(&profile, "deps/app-0a0a.cgu.first.rcgu.o", "old");
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH)
        .unwrap();
    write(&profile, "deps/app-0a0a.cgu.second.rcgu.o", "new");

    let report = sweep_to(&trees, None);

    assert_eq!(report.objects, 1, "{report:?}");
    assert!(!old.exists());
    assert!(profile.join("deps/app-0a0a.cgu.second.rcgu.o").exists());
    assert!(report.reclaimed.is_empty());
}

#[test]
fn making_room_for_a_ticket_keeps_leased_and_landing_builds() {
    let f = Fixture::new(2, 1);
    let trees = all_leased(&f, &[MB]);
    let worktrees = f.repo.join(".worktrees");
    write(&worktrees.join("tree-02"), "out/big", "unused build");
    write(&worktrees.join("landing-1"), "out/big", "tip build");

    let report = trees.free_unused_disk(100, || Some(50), &|| false).unwrap();

    assert_eq!(report.cleared, [id("tree-02")], "{report:?}");
    assert!(worktrees.join("tree-01/out/big").exists());
    assert!(worktrees.join("landing-1/out/big").exists());
}

/// Below the disk's floor with no free ticket tree, idle leased trees are cleared too.
#[test]
fn below_the_floor_idle_leased_trees_are_cleared_once_no_free_tree_is_left() {
    let f = Fixture::new(2, 1);
    let trees = all_leased(&f, &[MB, MB]);
    let reads = Cell::new(0);
    let report = trees
        .free_disk(100, || {
            reads.set(reads.get() + 1);
            Some(10)
        })
        .unwrap();

    assert!(report.reclaimed.contains(&id("tree-02")), "{report:?}");
    assert!(!f.repo.join(".worktrees/tree-02/out").exists());
    assert_eq!(state(&trees, "tree-02"), "leased");
}

/// Over the limit with no free tree, idle leased trees lose their incremental caches first, and
/// a tree is cleared whole only if that is not enough. On jarvis 39 of tree-05's 101 GB in
/// `rust/target/debug` were incremental caches, which cost only a slower next compile.
#[test]
fn incremental_caches_go_before_any_leased_tree_is_cleared() {
    let f = Fixture::new(2, 1);
    // The files are written after the leases, which seed one tree as a clone of another, so
    // the trees share no blocks and hold about 6 MB, 4 MB of it incremental caches.
    let trees = all_leased(&f, &[0, 0]);
    for tree in ["tree-01", "tree-02"] {
        let profile = f.repo.join(".worktrees").join(tree).join("out/debug");
        write(&profile, ".fingerprint/app-0a0a/bin-app", "");
        write(&profile, "deps/app-0a0a", &"x".repeat(MB));
        write(
            &profile,
            "incremental/app-1b1b/s-1/query-cache.bin",
            &"x".repeat(2 * MB),
        );
    }

    let report = sweep_to(&trees, Some(9 * MB as u64 / 2));

    assert!(report.incremental.contains(&id("tree-02")), "{report:?}");
    assert!(report.reclaimed.is_empty(), "{report:?}");
    assert!(report.after <= report.limit.unwrap(), "{report:?}");
    let profile = f.repo.join(".worktrees/tree-02/out/debug");
    assert!(!profile.join("incremental").exists());
    assert!(profile.join("deps/app-0a0a").exists());
    assert_eq!(state(&trees, "tree-02"), "leased");
}

/// On 2026-11-20 every tree was leased and running commands, and the disk stayed full while the
/// log said only what had been deleted. Below the floor with every tree busy, the pass deletes
/// nothing in them and names each tree and what runs in it, including a command that only waits
/// for the disk space this pass is failing to find. Once that command ends, the next pass clears
/// its tree.
#[test]
fn below_the_floor_with_every_tree_busy_the_pass_says_why_and_clears_the_first_to_go_idle() {
    let f = Fixture::new(2, 1);
    let trees = all_leased(&f, &[MB, MB]);
    let verb = |tree: &str, name: &str| {
        crate::steps::global().begin(
            crate::steps::Kind::Verb(name.to_owned()),
            crate::steps::Work {
                ticket: Some(TicketId::new("T.1").unwrap()),
                tree: Some(id(tree)),
                tree_path: Some(trees.config().path(&id(tree))),
                ..crate::steps::Work::default()
            },
        )
    };
    let running = verb("tree-01", "check");
    let waiting = verb("tree-02", "test");
    let entered = waiting.enter();
    let wait = crate::steps::waiting(format!(
        "{} 3 GB free, under the 30 GB a build needs",
        crate::build::DISK_WAIT
    ));

    let report = trees.free_disk(100, || Some(10)).unwrap();
    drop(wait);
    drop(entered);

    assert!(report.reclaimed.is_empty(), "{report:?}");
    let stuck = report.stuck.expect("the pass says what holds the space");
    assert_eq!((stuck.free, stuck.floor), (10, 100));
    let why = |tree: &str| {
        stuck
            .busy
            .iter()
            .find(|busy| busy.tree == id(tree))
            .map(|busy| busy.why.clone())
            .unwrap_or_default()
    };
    assert!(
        why("tree-01").contains("`check` for T.1 runs from it"),
        "{stuck:?}"
    );
    assert_eq!(
        why("tree-02"),
        "`test` for T.1 waits for disk space to start a build"
    );
    assert!(f.repo.join(".worktrees/tree-02/out/big").exists());

    waiting.finish();
    drop(waiting);
    let report = trees.free_disk(100, || Some(10)).unwrap();
    running.finish();

    assert_eq!(report.reclaimed, [id("tree-02")], "{report:?}");
    assert!(!f.repo.join(".worktrees/tree-02/out").exists());
    let stuck = report.stuck.expect("the disk is still below its floor");
    assert!(
        stuck.busy.iter().all(|busy| busy.tree != id("tree-02")),
        "{stuck:?}"
    );
}
