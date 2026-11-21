//! The disk loop sweeps leased trees that run nothing: their stale object files and replaced
//! builds. Current outputs are kept between commands unless the disk is below its emergency
//! free-space floor.
//!
//! Other tests in this crate register running verbs from `tree-01` with no path, which the
//! disk loop takes for a verb in this fixture's `tree-01` too, so these tests assert nothing
//! that depends on `tree-01` being idle.

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, channel};

use ignite_core::capacity::BYTES_PER_GB;
use ignite_os::system::DiskSpace;

use super::*;

const MB: usize = 1_000_000;
const SWEEP_TIMEOUT: Duration = Duration::from_secs(60);

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

/// Holds a sweep at the point before it deletes the build directory.
struct SweepBuilds {
    active: AtomicBool,
    paused: AtomicBool,
    forgetting: Sender<()>,
    finish_forgetting: Mutex<Receiver<()>>,
    next_build: Sender<()>,
    path: PathBuf,
}

impl Builds for SweepBuilds {
    fn ensure(&self, _: &TreeId, _: &str, _: &[String]) -> BuildResult {
        assert!(!self.paused.load(Ordering::SeqCst));
        assert!(!self.path.join("out/old").exists());
        write(&self.path, "out/next", "new build");
        self.next_build.send(()).unwrap();
        BuildResult::Current
    }

    fn pause(&self, _: &TreeId) -> bool {
        if self.active.load(Ordering::SeqCst) {
            return false;
        }
        assert!(!self.paused.swap(true, Ordering::SeqCst));
        true
    }

    fn resume(&self, _: &TreeId) {
        assert!(self.paused.swap(false, Ordering::SeqCst));
    }

    fn forget(&self, _: &TreeId) {
        self.forgetting.send(()).unwrap();
        self.finish_forgetting
            .lock()
            .unwrap()
            .recv_timeout(SWEEP_TIMEOUT)
            .unwrap();
    }
}

#[test]
fn a_sweep_finishes_before_the_tree_is_reset_and_leased_again() {
    let f = Fixture::with_landing(1, 0, 1);
    let tree = id("tree-01");
    let path = f.trees.config().path(&tree);
    let (forgetting, sweep_started) = channel();
    let (finish_sweep, finish_forgetting) = channel();
    let (next_build, next_build_done) = channel();
    let builds = Arc::new(SweepBuilds {
        active: AtomicBool::new(true),
        paused: AtomicBool::new(false),
        forgetting,
        finish_forgetting: Mutex::new(finish_forgetting),
        next_build,
        path: path.clone(),
    });
    let mut trees = Trees::new(
        f.trees.config().clone(),
        open_in_memory().unwrap(),
        f.clock.clone(),
        builds.clone(),
    );
    let steps = Arc::new(crate::steps::Steps::new(f.clock.clone()));
    Arc::get_mut(&mut trees.inner).unwrap().steps = steps.clone();
    trees.setup(&f.tip()).unwrap();
    assert!(matches!(
        trees.lease(f.request("T.1", false)).unwrap(),
        Lease::Ready(_)
    ));
    trees.record_build(&tree, &f.tip()).unwrap();
    write(&path, "out/old", "old build");

    let report = trees.free_disk(100, || Some(10)).unwrap();
    assert!(report.reclaimed.is_empty());
    assert!(path.join("out/old").exists());
    assert!(!builds.paused.load(Ordering::SeqCst));
    builds.active.store(false, Ordering::SeqCst);

    std::thread::scope(|scope| {
        let finish_sweep = finish_sweep;
        let sweep = scope.spawn(|| {
            trees
                .free_disk(100, || {
                    Some(if path.join("out/old").exists() {
                        10
                    } else {
                        100
                    })
                })
                .unwrap()
        });
        sweep_started.recv_timeout(SWEEP_TIMEOUT).unwrap();
        let Lease::Waiting(waiter) = trees.lease(f.request("T.2", true)).unwrap() else {
            panic!("expected the next ticket to wait for the same tree");
        };
        let release = steps.begin(
            crate::steps::Kind::Verb("release".to_owned()),
            crate::steps::Work {
                tree: Some(tree.clone()),
                tree_path: Some(path.clone()),
                ..crate::steps::Work::default()
            },
        );
        let release_steps = release.subscribe();
        let (release_trees, release_tree, tip) = (&trees, &tree, f.tip());
        let released = scope.spawn(move || {
            let _entered = release.enter();
            let result = release_trees.release_unsaved(release_tree, &tip, None);
            release.finish();
            result.unwrap();
        });
        assert_eq!(
            release_steps.recv_timeout(SWEEP_TIMEOUT).unwrap(),
            "waiting for checkout or disk cleanup in tree-01 to end"
        );
        assert!(waiter.wait(Duration::ZERO).is_none());
        assert!(path.join("out/old").is_file());
        assert_eq!(refs::branch(&path).unwrap().as_deref(), Some("work/T.1"));

        finish_sweep.send(()).unwrap();
        assert_eq!(sweep.join().unwrap().reclaimed, std::slice::from_ref(&tree));
        released.join().unwrap();
        let next = waiter.wait(SWEEP_TIMEOUT).unwrap().unwrap();
        assert_eq!(next.tree, tree);
        assert_eq!(next.branch, "work/T.2");
        next_build_done.recv_timeout(SWEEP_TIMEOUT).unwrap();
        assert_eq!(
            fs::read_to_string(path.join("out/next")).unwrap(),
            "new build"
        );
    });
}

#[test]
fn over_budget_sweeps_keep_the_working_set_until_the_lease_ends() {
    let f = Fixture::new(2, 1);
    let trees = all_leased(&f, &[MB, 3 * MB]);
    let tree = id("tree-02");
    let path = trees.config().path(&tree);
    // A lease's prebuild thread records the tree's build after the seed's mark is gone, so
    // `wait_until_seeded` does not wait for it. Record it here, as that thread does, so that the
    // build the sweeps must keep is the same whichever of the two runs first.
    assert!(trees.build(&tree, &f.tip(), &[]));
    let built = built_commit(&trees, tree.as_str());
    assert_eq!(built.as_deref(), Some(f.tip().as_str()));
    let forgotten = f.builds.forgotten();

    for (cap, available_gb, source) in [
        (Some(0), 120, DiskLimitSource::Cap),
        (None, 75, DiskLimitSource::Disk),
    ] {
        let disk = DiskSpace {
            total: 1_000 * BYTES_PER_GB,
            available: available_gb * BYTES_PER_GB,
        };
        for _ in 0..2 {
            let report = trees
                .hold_disk_under(|used| TreeConfig::disk_budget(Some(disk), Some(used), cap))
                .unwrap();
            assert_eq!(report.limit_source, Some(source));
            assert!(report.after > report.limit.unwrap(), "{report:?}");
            assert!(report.cleared.is_empty(), "{report:?}");
            assert!(report.incremental.is_empty(), "{report:?}");
            assert!(report.reclaimed.is_empty(), "{report:?}");
            assert!(path.join("out/big").exists());
            assert_eq!(built_commit(&trees, tree.as_str()), built);
            assert_eq!(f.builds.forgotten(), forgotten);
        }
    }

    trees.release_unsaved(&tree, &f.tip(), None).unwrap();
    let report = sweep_to(&trees, Some(2 * MB as u64));
    assert_eq!(report.cleared, [tree], "{report:?}");
    assert!(report.after <= report.limit.unwrap(), "{report:?}");
    assert!(!path.join("out").exists());
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

#[test]
fn the_floor_pass_can_reclaim_incremental_caches_kept_by_budget_sweeps() {
    let f = Fixture::new(2, 1);
    let trees = all_leased(&f, &[0, 0]);
    let profile = trees.config().path(&id("tree-02")).join("out/debug");
    write(&profile, ".fingerprint/app-0a0a/bin-app", "");
    write(&profile, "deps/app-0a0a", "build");
    let incremental = "incremental/app-1b1b/s-1/query-cache.bin";
    write(&profile, incremental, "cache");

    let report = sweep_to(&trees, Some(0));
    assert!(report.incremental.is_empty(), "{report:?}");
    assert!(profile.join(incremental).is_file());

    let report = trees
        .free_disk(100, || {
            Some(if profile.join(incremental).exists() {
                10
            } else {
                100
            })
        })
        .unwrap();
    assert_eq!(report.incremental, [id("tree-02")], "{report:?}");
    assert!(report.reclaimed.is_empty(), "{report:?}");
    assert!(!profile.join("incremental").exists());
    assert!(profile.join("deps/app-0a0a").exists());
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
