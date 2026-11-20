//! Seeding a compiling ticket's tree from a tree whose build is nearer the tip.

use std::fs::File;
use std::time::{Duration, SystemTime};

use super::*;

/// A Cargo target directory at `out` in `tree`: one unit whose dependency file names a file of
/// the tree by its full path, and one build script whose output names one.
fn fake_target(tree: &Path) {
    let root = tree.to_string_lossy().into_owned();
    let path = format!("{root}/a.txt");
    let mut dep = 1u32.to_le_bytes().to_vec();
    dep.push(2);
    dep.extend_from_slice(&u32::try_from(path.len()).unwrap().to_le_bytes());
    dep.extend_from_slice(path.as_bytes());
    let profile = tree.join("out/debug");
    fs::create_dir_all(profile.join(".fingerprint/foo-1")).unwrap();
    fs::write(profile.join(".fingerprint/foo-1/dep-lib-foo"), dep).unwrap();
    write(
        &profile,
        "build/foo-2/output",
        &format!("cargo:rerun-if-changed={path}\n"),
    );
    write(&profile, "deps/libfoo-1.rlib", "compiled");
    write(&profile, ".cargo-lock", "");
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

fn set_time(path: &Path, time: SystemTime) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

/// Whether copies in the temporary directory, where every fixture is, are clones, which seeding
/// needs. On Windows that is a Dev Drive (`ReFS`); a test that needs it passes there and is
/// skipped elsewhere, with a line that says so.
fn copies_are_clones_here() -> bool {
    use ignite_os::clones::Clones as _;
    let here = ignite_os::Native.copies_are_clones(&std::env::temp_dir());
    if !here {
        eprintln!("skipped: copies in the temporary directory are not clones");
    }
    here
}

/// Copies `from` to `to` as a seed does: a clone that keeps the source's modification time.
fn clone_file(from: &Path, to: &Path) {
    fs::copy(from, to).unwrap();
    set_time(to, modified(from));
}

/// tree-01 last built a commit behind the tip; tree-02, leased to another ticket, holds a build
/// at the tip. Returns the fixture and tree-02's path.
fn donor_at_the_tip() -> (Fixture, PathBuf) {
    let f = Fixture::new(2, 2);
    f.trees.record_build(&id("tree-01"), &f.commits[0]).unwrap();
    let other = f.lease("T.1", false);
    assert_eq!(other.tree.as_str(), "tree-02");
    fake_target(&other.path);
    f.trees.record_build(&other.tree, &f.tip()).unwrap();
    (f, other.path)
}

#[test]
fn a_compiling_lease_starts_from_a_copy_of_the_build_nearest_the_tip() {
    if !copies_are_clones_here() {
        return;
    }
    let (f, donor) = donor_at_the_tip();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    // `.gitignore` is the same in both trees; `a.txt` is edited in the donor and not committed.
    set_time(&donor.join(".gitignore"), old);
    write(&donor, "a.txt", "edited\n");
    set_time(&donor.join("a.txt"), old);
    let own = f.repo.join(".worktrees/tree-01");
    write(&own, "out/debug/.fingerprint/stale-9/lib-stale", "0");

    let lease = f.lease("T.2", true);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert_eq!(lease.tree.as_str(), "tree-01");
    let profile = lease.path.join("out/debug");
    assert_eq!(
        fs::read_to_string(profile.join("deps/libfoo-1.rlib")).unwrap(),
        "compiled"
    );
    assert!(
        !profile.join(".fingerprint/stale-9").exists(),
        "its own units are replaced"
    );
    let path = format!("{}/a.txt", lease.path.to_string_lossy());
    let dep = fs::read(profile.join(".fingerprint/foo-1/dep-lib-foo")).unwrap();
    assert_eq!(
        &dep[5..9],
        &u32::try_from(path.len()).unwrap().to_le_bytes()
    );
    assert_eq!(&dep[9..], path.as_bytes());
    assert_eq!(
        fs::read_to_string(profile.join("build/foo-2/output")).unwrap(),
        format!("cargo:rerun-if-changed={path}\n")
    );
    assert_eq!(modified(&lease.path.join(".gitignore")), old);
    assert!(
        modified(&lease.path.join("a.txt")) > old,
        "a file that differs is touched"
    );
    let built = f.trees.status().unwrap().trees;
    let built = built.iter().find(|t| t.tree.as_str() == "tree-01").unwrap();
    assert_eq!(built.built_commit.as_deref(), Some(f.tip().as_str()));
}

/// A tree that never built a product another tree holds would pass it on to be built cold, so
/// a donor that holds every product is taken although another is nearer the tip. The seeded
/// tree then holds what its donor held, so it can be a donor in turn.
#[test]
fn a_donor_holding_every_product_is_preferred_to_one_nearer_the_tip() {
    if !copies_are_clones_here() {
        return;
    }
    let (f, _) = donor_at_the_tip();
    let landing = f.repo.join(".worktrees/landing-1");
    fake_target(&landing);
    write(&landing, "out/debug/deps/marker", "landing-1");
    f.trees
        .record_build(&id("landing-1"), &f.commits[0])
        .unwrap();
    f.builds.hold(&id("tree-02"), "bin");
    f.builds.hold(&id("landing-1"), "bin");
    f.builds.hold(&id("landing-1"), "lint");

    let lease = f.lease("T.2", true);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert_eq!(lease.tree.as_str(), "tree-01");
    assert_eq!(
        fs::read_to_string(lease.path.join("out/debug/deps/marker")).unwrap(),
        "landing-1"
    );
    let held = f.builds.held_in(&lease.tree, &[]);
    assert_eq!(held.into_iter().collect::<Vec<_>>(), ["bin", "lint"]);
}

/// A leased tree's build was made from its ticket's edits, which the commit its row records
/// does not show. Its uncommitted files count as differing from the tip, so a free tree built a
/// commit behind the tip, with fewer files differing, is taken instead.
#[test]
fn a_leased_donor_whose_ticket_changed_more_files_is_passed_over() {
    if !copies_are_clones_here() {
        return;
    }
    let f = Fixture::new(2, 3);
    f.trees.record_build(&id("tree-01"), &f.commits[0]).unwrap();
    let other = f.lease("T.1", false);
    assert_eq!(other.tree.as_str(), "tree-02");
    fake_target(&other.path);
    f.trees.record_build(&other.tree, &f.tip()).unwrap();
    for n in 0..3 {
        write(&other.path, &format!("new-{n}.txt"), "edited\n");
    }
    let landing = f.repo.join(".worktrees/landing-1");
    fake_target(&landing);
    write(&landing, "out/debug/deps/marker", "landing-1");
    f.trees
        .record_build(&id("landing-1"), &f.commits[1])
        .unwrap();

    let lease = f.lease("T.2", true);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert_eq!(lease.tree.as_str(), "tree-01");
    assert_eq!(
        fs::read_to_string(lease.path.join("out/debug/deps/marker")).unwrap(),
        "landing-1"
    );
}

#[test]
fn a_donor_whose_cargo_lock_is_held_is_passed_over() {
    let (f, donor) = donor_at_the_tip();
    let lock = File::open(donor.join("out/debug/.cargo-lock")).unwrap();
    lock.try_lock().unwrap();
    let own = f.repo.join(".worktrees/tree-01");
    write(&own, "out/debug/.fingerprint/mine-1/lib-mine", "0");

    let lease = f.lease("T.2", true);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert!(lease.path.join("out/debug/.fingerprint/mine-1").exists());
    assert!(!lease.path.join("out/debug/deps/libfoo-1.rlib").exists());
}

/// The session's own commands build in its tree without waiting for the seed. A seed that
/// deleted the tree's target directories under such a build made it fail on missing files, so a
/// build holding Cargo's lock in the leased tree when the copy is ready keeps the tree's own
/// build, and the copy made beside the tree is deleted.
#[test]
fn a_build_running_in_the_leased_tree_keeps_its_own_build() {
    if !copies_are_clones_here() {
        return;
    }
    let (f, _donor) = donor_at_the_tip();
    let own = f.repo.join(".worktrees/tree-01");
    write(&own, "out/debug/.fingerprint/mine-1/lib-mine", "0");
    write(&own, "out/debug/.cargo-lock", "");
    let lock = File::open(own.join("out/debug/.cargo-lock")).unwrap();
    lock.try_lock().unwrap();

    let lease = f.lease("T.2", true);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert_eq!(lease.path, own);
    assert!(own.join("out/debug/.fingerprint/mine-1").exists());
    assert!(!own.join("out/debug/deps/libfoo-1.rlib").exists());
    assert!(!f.repo.join(".worktrees/.tree-01.seeding").exists());
}

/// Once the copy replaces the tree's target directories, the copy made beside the tree is gone
/// and the tree's own build with it.
#[test]
fn a_seed_swaps_in_the_copy_and_leaves_nothing_beside_the_tree() {
    if !copies_are_clones_here() {
        return;
    }
    let (f, _donor) = donor_at_the_tip();
    let own = f.repo.join(".worktrees/tree-01");
    write(&own, "out/debug/.fingerprint/mine-1/lib-mine", "0");

    let lease = f.lease("T.2", true);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert!(own.join("out/debug/deps/libfoo-1.rlib").exists());
    assert!(!own.join("out/debug/.fingerprint/mine-1").exists());
    assert!(!f.repo.join(".worktrees/.tree-01.seeding").exists());
}

/// Builds that note, when a tree's builds are forgotten, whether its `out` directory was still
/// there: a tree's builds must be forgotten before its build directories are deleted, or a
/// restart partway through the deletion leaves the record of builds whose files are gone.
struct Watching {
    root: PathBuf,
    forgotten: std::sync::Mutex<Vec<(TreeId, bool)>>,
}

impl ignite_core::traits::Builds for Watching {
    fn ensure(
        &self,
        _tree: &TreeId,
        _product: &str,
        _packages: &[String],
    ) -> ignite_core::traits::BuildResult {
        ignite_core::traits::BuildResult::Current
    }

    fn forget(&self, tree: &TreeId) {
        let out = self.root.join(tree.as_str()).join("out");
        self.forgotten
            .lock()
            .unwrap()
            .push((tree.clone(), out.exists()));
    }
}

/// `f`'s trees again, over a new database, with `builds` and `disk_limit`.
fn trees_with(
    f: &Fixture,
    builds: Arc<dyn ignite_core::traits::Builds>,
    disk_limit: Option<u64>,
) -> Trees {
    let mut config = f.trees.config().clone();
    config.disk_limit = disk_limit;
    let trees = Trees::new(
        config,
        open_in_memory().unwrap(),
        Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        )),
        builds,
    );
    trees.setup(&f.tip()).unwrap();
    trees
}

fn watching(f: &Fixture) -> Arc<Watching> {
    Arc::new(Watching {
        root: f.repo.join(".worktrees"),
        forgotten: std::sync::Mutex::default(),
    })
}

/// A free tree whose files are clones of a leased tree's frees almost nothing when deleted, so
/// the disk loop keeps it, as a donor for the next lease, and clears a free tree whose bytes are
/// its own, although that one was used more recently.
#[test]
fn a_free_tree_whose_blocks_a_clone_shares_is_kept_and_one_with_its_own_is_cleared() {
    if !copies_are_clones_here() {
        return;
    }
    let f = Fixture::new(3, 1);
    let builds = watching(&f);
    let probe = trees_with(&f, builds.clone(), None);
    let leased = match probe.lease(f.request("T.1", false)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };
    // The free trees are cleared in the order of their ids, since neither was used.
    let mut free: Vec<String> = ["tree-01", "tree-02", "tree-03"]
        .into_iter()
        .filter(|t| *t != leased.tree.as_str())
        .map(str::to_owned)
        .collect();
    free.sort();
    let (donor, own) = (
        f.repo.join(".worktrees").join(&free[0]),
        f.repo.join(".worktrees").join(&free[1]),
    );
    write(&donor, "out/big", &"x".repeat(4_000_000));
    fs::create_dir_all(leased.path.join("out")).unwrap();
    clone_file(&donor.join("out/big"), &leased.path.join("out/big"));
    write(&own, "out/own", &"y".repeat(2_000_000));
    let total = probe.hold_disk(0).unwrap().before;
    assert!(total < 7_000_000, "the clone counts once: {total}");

    let limited = trees_with(&f, builds.clone(), Some(total - 1_000_000));
    let held = match limited.lease(f.request("T.1", false)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };
    assert_eq!(held.tree, leased.tree);
    let report = limited.hold_disk(0).unwrap();

    assert_eq!(report.cleared, [id(&free[1])], "{report:?}");
    assert!(report.after <= report.limit.unwrap(), "{report:?}");
    assert!(donor.join("out/big").exists());
    assert!(!own.join("out").exists());
    assert_eq!(
        *builds.forgotten.lock().unwrap(),
        [(id(&free[1]), true)],
        "forgotten before the deletion"
    );
}

/// The disk loop forgets a cleared tree's builds before it deletes its build directories.
#[test]
fn a_cleared_tree_is_forgotten_before_its_directories_are_deleted() {
    let f = Fixture::new(1, 1);
    write(
        &f.repo.join(".worktrees/tree-01"),
        "out/big",
        &"x".repeat(100_000),
    );
    let builds = watching(&f);
    let trees = trees_with(&f, builds.clone(), Some(1));

    let report = trees.hold_disk(0).unwrap();

    assert_eq!(report.cleared, [id("tree-01")]);
    assert_eq!(*builds.forgotten.lock().unwrap(), [(id("tree-01"), true)]);
    assert!(!f.repo.join(".worktrees/tree-01/out").exists());
}

/// Seeding forgets the leased tree's builds before it replaces the tree's target directories,
/// so that a copy that fails partway, which deletes them, leaves no record of them either.
#[test]
fn a_seeded_tree_is_forgotten_before_its_own_build_is_replaced() {
    if !copies_are_clones_here() {
        return;
    }
    let f = Fixture::new(2, 2);
    let builds = watching(&f);
    let trees = trees_with(&f, builds.clone(), None);
    trees.record_build(&id("tree-01"), &f.commits[0]).unwrap();
    let other = match trees.lease(f.request("T.1", false)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };
    fake_target(&other.path);
    trees.record_build(&other.tree, &f.tip()).unwrap();
    write(
        &f.repo.join(".worktrees/tree-01"),
        "out/debug/.fingerprint/mine-1/lib-mine",
        "0",
    );

    let lease = match trees.lease(f.request("T.2", true)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert_eq!(lease.tree.as_str(), "tree-01");
    assert!(lease.path.join("out/debug/deps/libfoo-1.rlib").exists());
    assert_eq!(*builds.forgotten.lock().unwrap(), [(id("tree-01"), true)]);
}

/// Builds whose `forget`, which the seed calls partway through the copy, waits while `held`
/// is set, and that record each call in order.
#[derive(Default)]
struct Held {
    /// Whether `forget` waits, and every call so far.
    state: std::sync::Mutex<(bool, Vec<String>)>,
    changed: std::sync::Condvar,
}

impl Held {
    fn set_held(&self, held: bool) {
        self.state.lock().unwrap().0 = held;
        self.changed.notify_all();
    }

    fn record(&self, call: String) -> std::sync::MutexGuard<'_, (bool, Vec<String>)> {
        let mut state = self.state.lock().unwrap();
        state.1.push(call);
        self.changed.notify_all();
        state
    }

    /// Waits, without a timeout, until `call` has been made.
    fn until_called(&self, call: &str) -> Vec<String> {
        let mut state = self.state.lock().unwrap();
        while !state.1.iter().any(|c| c == call) {
            state = self.changed.wait(state).unwrap();
        }
        state.1.clone()
    }
}

impl ignite_core::traits::Builds for Held {
    fn ensure(
        &self,
        tree: &TreeId,
        product: &str,
        _packages: &[String],
    ) -> ignite_core::traits::BuildResult {
        drop(self.record(format!("ensure {product} in {tree}")));
        ignite_core::traits::BuildResult::Current
    }

    fn forget(&self, tree: &TreeId) {
        let mut state = self.record(format!("forget {tree}"));
        while state.0 {
            state = self.changed.wait(state).unwrap();
        }
    }
}

/// A compiling lease returns while its tree is still being seeded, and the copy runs on the
/// thread that prebuilds the tree afterwards. A build asked for during the copy waits for it and
/// names the donor.
#[test]
fn a_compiling_lease_returns_before_its_seed_and_a_build_waits_for_the_seed() {
    if !copies_are_clones_here() {
        return;
    }
    let f = Fixture::new(2, 2);
    let builds = Arc::new(Held::default());
    let trees = trees_with(&f, builds.clone(), None);
    trees.record_build(&id("tree-01"), &f.commits[0]).unwrap();
    let other = match trees.lease(f.request("T.1", false)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };
    fake_target(&other.path);
    trees.record_build(&other.tree, &f.tip()).unwrap();
    builds.set_held(true);

    let lease = match trees.lease(f.request("T.2", true)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };

    // The seed is held partway, after the lease returned.
    assert_eq!(lease.tree.as_str(), "tree-01");
    builds.until_called("forget tree-01");
    let steps = crate::steps::Steps::new(Arc::new(ignite_core::time::SystemClock));
    let work = steps.begin(
        crate::steps::Kind::Verb("test".to_owned()),
        crate::steps::Work::default(),
    );
    let waiter = {
        let (path, work) = (lease.path.clone(), Arc::clone(&work));
        std::thread::spawn(move || {
            let _entered = work.enter();
            super::super::wait_until_seeded(&path)
        })
    };
    while work.waits().is_empty() {
        assert!(!waiter.is_finished(), "{:?}", waiter.join());
        std::thread::yield_now();
    }
    assert_eq!(
        work.waits(),
        ["tree-01's build directories to be copied from tree-02"]
    );

    builds.set_held(false);

    waiter.join().unwrap().unwrap();
    assert!(lease.path.join("out/debug/deps/libfoo-1.rlib").exists());
    let calls = builds.until_called("ensure bin in tree-01");
    assert_eq!(calls, ["forget tree-01", "ensure bin in tree-01"]);
}

/// Cargo leaves some outputs read-only. The seed copy keeps that mode, and used to fail with
/// "Permission denied" on macOS when it opened the copy for writing to set its modification time.
#[test]
fn a_read_only_file_is_copied_with_its_modification_time() {
    let dir = std::env::temp_dir().join(format!("ignite-readonly-copy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let (from, to) = (dir.join("from"), dir.join("to"));
    fs::create_dir_all(&from).unwrap();
    let file = from.join("output");
    fs::write(&file, "compiled").unwrap();
    let earlier = SystemTime::now() - Duration::from_secs(3600);
    set_time(&file, earlier);
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();

    crate::trees::seed::clone_dir(&from, &to).unwrap();

    assert_eq!(modified(&to.join("output")), earlier);
    assert!(
        fs::metadata(to.join("output"))
            .unwrap()
            .permissions()
            .readonly()
    );
    // Remove the read-only files' directory last; deleting needs only the directory's rights.
    fs::remove_dir_all(&dir).unwrap();
}

/// Under load most leased trees are building, and a seed whose best donor held Cargo's lock
/// ended without a copy. It goes on to the next donor.
#[test]
fn a_seed_whose_best_donor_is_building_copies_the_next() {
    if !copies_are_clones_here() {
        return;
    }
    let (f, donor) = donor_at_the_tip();
    let lock = File::open(donor.join("out/debug/.cargo-lock")).unwrap();
    lock.try_lock().unwrap();
    let landing = f.repo.join(".worktrees/landing-1");
    fake_target(&landing);
    write(&landing, "out/debug/deps/marker", "landing-1");
    f.trees.record_build(&id("landing-1"), &f.tip()).unwrap();
    // tree-02 holds a product the others lack, so it ranks first.
    f.builds.hold(&id("tree-02"), "bin");

    let lease = f.lease("T.2", true);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert_eq!(lease.tree.as_str(), "tree-01");
    assert_eq!(
        fs::read_to_string(lease.path.join("out/debug/deps/marker")).unwrap(),
        "landing-1"
    );
}

/// A release restarted the daemon seconds after a lease started its seed, the seed died with
/// the old daemon, and the ticket's first test compiled every crate from nothing. The seed
/// directory beside the tree outlives the daemon, and the next daemon starts that seed again.
#[test]
fn a_seed_a_restart_cut_short_is_started_again() {
    if !copies_are_clones_here() {
        return;
    }
    let (f, _donor) = donor_at_the_tip();
    // A lease that started no seed stands for one whose seed died with its daemon.
    let lease = f.lease("T.2", false);
    assert_eq!(lease.tree.as_str(), "tree-01");
    let marker = f.repo.join(".worktrees/.tree-01.seeding");
    fs::create_dir_all(marker.join("new/out")).unwrap();
    let other = f.repo.join(".worktrees/.landing-1.seeding");
    fs::create_dir_all(&other).unwrap();

    assert_eq!(f.trees.resume_seeds().unwrap(), [id("tree-01")]);
    super::super::wait_until_seeded(&lease.path).unwrap();

    assert!(lease.path.join("out/debug/deps/libfoo-1.rlib").exists());
    assert!(!marker.exists());
    assert!(
        !other.exists(),
        "a tree not leased to a ticket is not seeded"
    );
}
