use super::*;

/// A dependency file as Cargo writes it: each path's length in four little-endian bytes, then
/// the path, with a kind byte before each.
fn encoded(paths: &[&str]) -> Vec<u8> {
    let mut out = u32::try_from(paths.len()).unwrap().to_le_bytes().to_vec();
    for path in paths {
        out.push(2);
        out.extend_from_slice(&u32::try_from(path.len()).unwrap().to_le_bytes());
        out.extend_from_slice(path.as_bytes());
    }
    out
}

#[test]
fn a_path_under_the_donor_moves_to_the_tree_with_its_length() {
    let data = encoded(&["src/lib.rs", "/w/tree-03/data/store.tsv", "/w/tree-030/x"]);
    let new = rewrite_dep_info(&data, b"/w/tree-03/", b"/w/landing-1/").unwrap();
    assert_eq!(
        new,
        encoded(&["src/lib.rs", "/w/landing-1/data/store.tsv", "/w/tree-030/x"])
    );
}

#[test]
fn a_prefix_that_is_not_where_a_path_starts_is_refused() {
    let mut data = encoded(&["src/lib.rs"]);
    data.extend_from_slice(b"junk /w/tree-03/x");
    assert_eq!(
        rewrite_dep_info(&data, b"/w/tree-03/", b"/w/tree-05/"),
        None
    );
}

#[test]
fn text_is_replaced_everywhere() {
    assert_eq!(
        replace(b"a=/w/t1/x b=/w/t1/y", b"/w/t1/", b"/w/tree-1/"),
        b"a=/w/tree-1/x b=/w/tree-1/y".to_vec()
    );
}

/// A fresh directory under the temporary directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-seed-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A seed failed with "No such file or directory" (os error 2) when a file of the donor's
/// target directory was deleted while it was copied, as a leased donor's tests delete their
/// scratch files. Such a file is left out and the rest is copied.
#[test]
fn a_file_deleted_during_the_copy_is_left_out() {
    let dir = scratch("vanish");
    let from = dir.join("from");
    write(&from.join("deps/libkept.rlib"), "kept");
    write(&from.join("deps/scratch.txt"), "deleted during the copy");
    write(&from.join("deps/sub/inner"), "deleted with its directory");

    copy::walk(&from, &dir.join("to"), &|path| {
        if path.ends_with("scratch.txt") {
            fs::remove_file(path).unwrap();
        } else if path.ends_with("sub") {
            fs::remove_dir_all(path).unwrap();
        }
    })
    .unwrap();

    assert_eq!(
        fs::read_to_string(dir.join("to/deps/libkept.rlib")).unwrap(),
        "kept"
    );
    assert!(!dir.join("to/deps/scratch.txt").exists());
    let _ = fs::remove_dir_all(&dir);
}

/// A seed failed with "Directory not empty" (os error 66) when a build that started during the
/// swap made the tree's target directory again (Cargo writes `CACHEDIR.TAG` before it waits for
/// its lock) before the copy was moved in. What was made is moved aside and the copy goes in.
#[test]
fn a_target_directory_made_during_the_swap_is_moved_aside() {
    let dir = scratch("swap");
    let (new, to, old) = (
        dir.join("new/out"),
        dir.join("tree/out"),
        dir.join("old/out"),
    );
    write(&new.join("debug/deps/libfoo.rlib"), "copied");
    write(&to.join("CACHEDIR.TAG"), "made by a waiting build");

    swap::swap_in(&new, &to, &old).unwrap();

    assert_eq!(
        fs::read_to_string(to.join("debug/deps/libfoo.rlib")).unwrap(),
        "copied"
    );
    assert!(!to.join("CACHEDIR.TAG").exists());
    assert!(old.join("made-during-seed-1/CACHEDIR.TAG").exists());
    assert!(!new.exists());
    let _ = fs::remove_dir_all(&dir);
}

/// A seed failed with "Permission denied" (os error 13) on a file it could not read. Cargo's
/// `tmp` directory and nextest's hold what tests write after the build has released Cargo's
/// lock, so only the profile directories and the root's files are copied.
#[cfg(unix)]
#[test]
fn only_the_profile_directories_and_root_files_of_a_target_are_copied() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("target");
    let from = dir.join("from");
    write(&from.join("CACHEDIR.TAG"), "tag");
    write(&from.join("debug/.fingerprint/foo-1/lib-foo"), "0");
    write(&from.join("debug/deps/libfoo.rlib"), "compiled");
    write(
        &from.join("aarch64-apple-ios-sim/debug/.fingerprint/bar-1/lib-bar"),
        "0",
    );
    write(&from.join("nextest/archive"), "archive");
    let unreadable = from.join("tmp/test-scratch/secret");
    write(&unreadable, "a test made this unreadable");
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

    copy::clone_target(&from, &dir.join("to")).unwrap();

    let to = dir.join("to");
    assert!(to.join("CACHEDIR.TAG").exists());
    assert!(to.join("debug/deps/libfoo.rlib").exists());
    assert!(
        to.join("aarch64-apple-ios-sim/debug/.fingerprint/bar-1/lib-bar")
            .exists()
    );
    assert!(!to.join("tmp").exists());
    assert!(!to.join("nextest").exists());
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
    let _ = fs::remove_dir_all(&dir);
}

/// The file-by-file copy, the fallback where a directory cannot be cloned in one call, keeps
/// each file's modification time, as Cargo needs.
#[test]
fn the_file_by_file_copy_keeps_modification_times() {
    let dir = scratch("walk-times");
    let file = dir.join("from/deps/libfoo.rlib");
    write(&file, "compiled");
    let earlier = SystemTime::now() - Duration::from_hours(3);
    File::open(&file).unwrap().set_modified(earlier).unwrap();

    copy::walk(&dir.join("from"), &dir.join("to"), &|_| {}).unwrap();

    let copied = fs::metadata(dir.join("to/deps/libfoo.rlib")).unwrap();
    assert_eq!(copied.modified().unwrap(), earlier);
    let _ = fs::remove_dir_all(&dir);
}
