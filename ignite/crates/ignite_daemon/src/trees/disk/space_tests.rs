//! Measuring directories whose files are clones of each other's.

use std::fs;
use std::path::PathBuf;

use super::*;

/// A directory under the system temp directory, deleted when dropped. The temporary directory
/// is on the startup disk, which is APFS on macOS.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-space-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        fs::create_dir_all(dir.join("c")).unwrap();
        Scratch(dir)
    }

    fn measure(&self) -> Space {
        Space::measure(["a", "b", "c"].map(|t| (id(t), vec![self.0.join(t)])))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn id(name: &str) -> TreeId {
    TreeId::new(format!("tree-{name}")).unwrap()
}

const MB: u64 = 1_000_000;

/// Tree a holds a file and tree b a clone of it; tree c a file of its own. The clone's blocks
/// count once, and deleting either tree of the pair frees almost nothing while the other stays.
/// Skipped where copies in the temporary directory are not clones (Windows off a Dev Drive).
#[test]
fn a_clone_counts_once_and_frees_its_blocks_only_with_its_source() {
    use ignite_os::clones::Clones as _;
    if !ignite_os::Native.copies_are_clones(&ignite_core::test_files::temp_dir()) {
        eprintln!("skipped: copies in the temporary directory are not clones");
        return;
    }
    let s = Scratch::new("clone");
    fs::write(s.0.join("a/big"), "x".repeat(4_000_000)).unwrap();
    // `fs::copy` clones on APFS and on ReFS; a seed keeps the source's modification time.
    fs::copy(s.0.join("a/big"), s.0.join("b/big")).unwrap();
    let time = fs::metadata(s.0.join("a/big")).unwrap().modified().unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(s.0.join("b/big"))
        .unwrap()
        .set_modified(time)
        .unwrap();
    fs::write(s.0.join("c/own"), "y".repeat(1_000_000)).unwrap();

    let mut space = s.measure();

    let total = space.total();
    assert!((5 * MB..6 * MB).contains(&total), "{total}");
    let own: Vec<u64> = space.own().into_iter().map(|(_, bytes)| bytes).collect();
    assert!(own[0] < MB && own[1] < MB, "{own:?}");
    assert!(own[2] >= MB, "{own:?}");
    let pair = HashSet::from([id("b")]);
    assert!(space.freed_with(&id("a"), &pair) >= 4 * MB);
    assert!(!space.group_can_free(&id("a"), &HashSet::new(), MB));
    assert!(space.group_can_free(&id("a"), &pair, MB));
    assert!(space.clear(&id("a")) < MB);
    assert!(space.clear(&id("b")) >= 4 * MB);
    assert!(space.holds_any(&id("c")));
    assert!(!space.holds_any(&id("a")));
}

/// Without clones, every tree's bytes are its own and the total is their sum.
#[test]
fn plain_files_count_in_their_own_tree() {
    let s = Scratch::new("plain");
    fs::write(s.0.join("a/big"), "x".repeat(2_000_000)).unwrap();
    fs::write(s.0.join("b/big"), "x".repeat(2_000_000)).unwrap();

    let space = s.measure();

    let own: Vec<u64> = space.own().into_iter().map(|(_, bytes)| bytes).collect();
    assert!(own[0] >= 2 * MB && own[1] >= 2 * MB, "{own:?}");
    assert_eq!(space.total(), own.iter().sum::<u64>());
}
