//! Deleting the object files that earlier compiles of a unit left in a profile's `deps` folder.
//!
//! On 2026-11-20 jarvis's disk filled with the trees' build directories at 150 to 200 GB each.
//! Half of a 113 GB tree was object files in `deps`: rustc names each codegen unit's object
//! `<crate>-<hash>.<codegen unit>.<invocation>.rcgu.o`, where the last part is new for every
//! compile, and with split debug info (the macOS default) it leaves them beside the binary,
//! which reads its debug info from them. A test binary rebuilt in place under the same
//! `<crate>-<hash>` therefore gains a full set of objects at every compile and loses none: one
//! unit had 4,370 objects, 19 compiles of 230 codegen units.
//!
//! Only the newest object of each codegen unit is one the current binary can name, so the
//! others are deleted. A compile that reuses a codegen unit from the incremental cache writes no
//! new object for it, and that unit's newest object is the one the binary names. Deleting an
//! older object loses nothing a build reads: Cargo's fingerprints do not name these files, and
//! rustc's incremental cache keeps its own copies.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{UNIT_FOLDERS, profile_dirs};

/// What [`sweep_objects`] deleted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Swept {
    pub files: u64,
    pub bytes: u64,
}

impl std::ops::AddAssign for Swept {
    fn add_assign(&mut self, other: Swept) {
        self.files += other.files;
        self.bytes += other.bytes;
    }
}

/// One object file: when it was written and changed, its name, its file ID and its bytes.
struct Object {
    path: PathBuf,
    newest: (SystemTime, SystemTime, String),
    inode: u64,
    bytes: u64,
}

/// Deletes the objects in each profile directory of the Cargo target directory `target` that a
/// newer object of the same unit and codegen unit replaced. A directory that is not a target
/// directory loses nothing.
pub(super) fn sweep_objects(target: &Path) -> Swept {
    let mut swept = Swept::default();
    for profile in profile_dirs(target) {
        for folder in UNIT_FOLDERS {
            swept += sweep_folder(&profile.join(folder));
        }
    }
    swept
}

fn sweep_folder(folder: &Path) -> Swept {
    let mut swept = Swept::default();
    if !fs::symlink_metadata(folder).is_ok_and(|meta| meta.is_dir()) {
        return swept;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return swept;
    };
    let mut groups: HashMap<String, Vec<Object>> = HashMap::new();
    for entry in entries.filter_map(io::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(key) = codegen_unit(&name) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let object = Object {
            path: entry.path(),
            newest: (
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                changed(&meta),
                name.clone(),
            ),
            inode: inode(&meta),
            bytes: allocated(&meta),
        };
        groups.entry(key.to_owned()).or_default().push(object);
    }
    for mut objects in groups.into_values() {
        objects.sort_by(|a, b| a.newest.cmp(&b.newest));
        let Some(kept) = objects.pop() else {
            continue;
        };
        for object in objects {
            // A hard link to the kept object frees nothing, and may be the name the binary
            // records, so it stays.
            if object.inode != 0 && object.inode == kept.inode {
                continue;
            }
            if fs::remove_file(&object.path).is_ok() {
                swept.files += 1;
                swept.bytes += object.bytes;
            }
        }
    }
    swept
}

/// The unit and codegen unit an object file belongs to, `<crate>-<hash>.<codegen unit>`, for a
/// name of the form `<crate>-<hash>.<codegen unit>.<invocation>.rcgu.o`. Any other name, such as
/// an older rustc's `<crate>-<hash>.<codegen unit>.rcgu.o`, which every compile overwrites, is
/// `None`.
fn codegen_unit(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(".rcgu.o")?;
    let (key, invocation) = stem.rsplit_once('.')?;
    let (unit, codegen) = key.split_once('.')?;
    let plain = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric());
    (unit.contains('-') && plain(codegen) && plain(invocation)).then_some(key)
}

#[cfg(unix)]
fn changed(meta: &fs::Metadata) -> SystemTime {
    use std::os::unix::fs::MetadataExt;
    let seconds = u64::try_from(meta.ctime()).unwrap_or(0);
    let nanos = u32::try_from(meta.ctime_nsec()).unwrap_or(0);
    SystemTime::UNIX_EPOCH + std::time::Duration::new(seconds, nanos)
}

#[cfg(windows)]
fn changed(meta: &fs::Metadata) -> SystemTime {
    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(unix)]
fn inode(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}

/// Windows reports no file ID without opening the file; 0 never matches.
#[cfg(windows)]
fn inode(_meta: &fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn allocated(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

#[cfg(windows)]
fn allocated(meta: &fs::Metadata) -> u64 {
    meta.len()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn an_object_name_gives_its_unit_and_codegen_unit() {
        assert_eq!(
            codegen_unit("game_cli-f136ed0f1a6c209b.2jmbaw4or42eutn680yz5hrjl.15k2fva.rcgu.o"),
            Some("game_cli-f136ed0f1a6c209b.2jmbaw4or42eutn680yz5hrjl")
        );
        assert_eq!(codegen_unit("serde-1a2b.serde.3c4d-cgu.0.rcgu.o"), None);
        assert_eq!(codegen_unit("libserde-1a2b.rlib"), None);
        assert_eq!(codegen_unit("serde-1a2b.d"), None);
    }

    /// Three compiles of one test binary under one unit name: the first two wrote both codegen
    /// units, the last only `cgub`. Each codegen unit keeps its newest object, so `cgua` keeps
    /// the second compile's.
    #[test]
    fn each_codegen_unit_keeps_only_its_newest_object() {
        let target = ignite_core::test_files::temp_dir()
            .join(format!("ignite-objects-{}", std::process::id()));
        let _ = fs::remove_dir_all(&target);
        let deps = target.join("debug/deps");
        fs::create_dir_all(target.join("debug/.fingerprint")).unwrap();
        fs::create_dir_all(&deps).unwrap();
        let write = |name: &str, age: u64| {
            let path = deps.join(name);
            fs::write(&path, "x".repeat(10_000)).unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(age))
                .unwrap();
        };
        for (invocation, age) in [("first", 100), ("second", 200)] {
            write(&format!("app-0a0a.cgua.{invocation}.rcgu.o"), age);
            write(&format!("app-0a0a.cgub.{invocation}.rcgu.o"), age);
        }
        write("app-0a0a.cgub.third.rcgu.o", 300);
        write("app-0a0a", 300);
        write("other-0b0b.cgua.first.rcgu.o", 100);

        let swept = sweep_objects(&target);

        let mut left: Vec<String> = fs::read_dir(&deps)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let _ = fs::remove_dir_all(&target);
        assert_eq!(swept.files, 3);
        assert!(swept.bytes > 0 || cfg!(windows));
        assert_eq!(
            left,
            [
                "app-0a0a",
                "app-0a0a.cgua.second.rcgu.o",
                "app-0a0a.cgub.third.rcgu.o",
                "other-0b0b.cgua.first.rcgu.o",
            ]
        );
    }
}
