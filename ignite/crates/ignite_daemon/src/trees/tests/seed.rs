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
    File::open(path).unwrap().set_modified(time).unwrap();
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

#[cfg(target_os = "macos")]
#[test]
fn a_compiling_lease_starts_from_a_copy_of_the_build_nearest_the_tip() {
    let (f, donor) = donor_at_the_tip();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    // `.gitignore` is the same in both trees; `a.txt` is edited in the donor and not committed.
    set_time(&donor.join(".gitignore"), old);
    write(&donor, "a.txt", "edited\n");
    set_time(&donor.join("a.txt"), old);
    let own = f.repo.join(".worktrees/tree-01");
    write(&own, "out/debug/.fingerprint/stale-9/lib-stale", "0");

    let lease = f.lease("T.2", true);

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

#[test]
fn a_donor_whose_cargo_lock_is_held_is_passed_over() {
    let (f, donor) = donor_at_the_tip();
    let lock = File::open(donor.join("out/debug/.cargo-lock")).unwrap();
    lock.try_lock().unwrap();
    let own = f.repo.join(".worktrees/tree-01");
    write(&own, "out/debug/.fingerprint/mine-1/lib-mine", "0");

    let lease = f.lease("T.2", true);

    assert!(lease.path.join("out/debug/.fingerprint/mine-1").exists());
    assert!(!lease.path.join("out/debug/deps/libfoo-1.rlib").exists());
}
