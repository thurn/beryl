//! Holding the trees under the machine's disk limit (`trees_disk_gb` in `ignite.toml`), and
//! under whatever size leaves the disk the free space the daemon keeps.
//!
//! The build directories are what grows. Each change to a crate's dependencies, features or
//! profile makes Cargo build that crate, and every crate above it, under a new hash beside the
//! old one, and nothing deletes the old one. So the daemon first deletes builds that a newer
//! build of the same unit replaced, and then, if the trees are still over the limit, the whole
//! build directories of the free trees used longest ago. Files that are clones of another tree's
//! count their shared blocks once (`disk/space.rs`).
//!
//! Before measuring, the daemon deletes the object files that earlier compiles of a unit left
//! behind (`disk/objects.rs`), which held half of a large tree's bytes.
//!
//! Free trees are marked `resetting` while their files are deleted, so no lease can take one half
//! swept, and each row is put back as it was afterwards, so the order of last use survives the
//! sweep. Leased trees are swept too while nothing runs in them, and cleared whole, largest
//! first, once no free tree is left to clear (`disk/leased.rs`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignite_core::ids::TreeId;
use ignite_core::traits::{Error, Result};
use ignite_store::local::{rows, trees as store};
use serde::Serialize;

use super::{Trees, store_error};
use space::Space;

mod leased;
mod objects;
mod space;
#[cfg(test)]
mod tests;

/// The bytes a free tree's deletion must free alone for the first pass of [`Trees::hold_disk`]
/// to clear it. A tree whose files are all clones of another tree's still writes its own lock
/// files and Cargo's small records, a few megabytes; a tree that built a ticket's change holds
/// gigabytes of its own.
const OWN_BYTES: u64 = 1_000_000_000;

/// The folder of per-unit fingerprints in each profile directory of a Cargo target directory,
/// which is how such a directory is recognised among a product's outputs.
const FINGERPRINTS: &str = ".fingerprint";
/// The folders of a profile directory that hold a unit's files, named `<crate>-<hash>` with
/// optional prefixes and extensions, beside its fingerprint folder.
const UNIT_FOLDERS: [&str; 2] = ["deps", "build"];
/// The fields of a fingerprint's JSON that tell two units of one crate apart, besides their
/// dependencies: two top-level builds that differ only in dependencies are one unit built twice.
const UNIT_FIELDS: [&str; 5] = ["features", "profile", "target", "compile_kind", "rustflags"];

/// What [`Trees::hold_disk`] found and deleted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DiskReport {
    /// Bytes in the trees' build directories before and after.
    pub before: u64,
    pub after: u64,
    pub limit: Option<u64>,
    /// Replaced builds deleted, as unit folder paths.
    pub superseded: Vec<PathBuf>,
    /// Free trees whose build directories were deleted whole.
    pub cleared: Vec<TreeId>,
    /// Leased trees, idle at the time, whose build directories were deleted whole.
    pub reclaimed: Vec<TreeId>,
    /// Object files that newer compiles of the same codegen unit replaced, deleted before the
    /// trees were measured, and the bytes they held (`disk/objects.rs`).
    pub objects: u64,
    pub object_bytes: u64,
}

impl Trees {
    /// The bytes that deleting each configured tree's build directories alone would free: the
    /// bytes they share with clones in another tree are not counted ([`Space`]).
    pub fn disk_usage(&self) -> Vec<(TreeId, u64)> {
        self.measure().own()
    }

    /// Measures every configured tree's build directories together.
    fn measure(&self) -> Space {
        let config = &self.inner.config;
        Space::measure(config.configured().into_iter().map(|(id, _)| {
            let path = config.path(&id);
            let dirs = config.keep.get().iter().map(|dir| path.join(dir)).collect();
            (id, dirs)
        }))
    }

    /// Brings the trees under the disk limit, if they are over it. `short` is how many bytes
    /// the disk lacks of the free space the daemon keeps; the trees are then held that many
    /// bytes under their current size as well, since a limit set when the disk had room says
    /// nothing about how full the rest of the disk has become since.
    ///
    /// Free trees are cleared in two passes, each in the order of last use. The first clears
    /// only trees whose deletion alone frees at least [`OWN_BYTES`]. A tree seeded from another
    /// holds clones of the donor's files, and deleting either frees almost nothing while the
    /// other keeps the shared blocks, so a donor is kept while a tree that holds bytes of its
    /// own can go instead. The second pass clears any free tree whose bytes would be freed if
    /// every free tree went, which takes the clones that only free trees share.
    ///
    /// Leased trees that run nothing are swept of replaced builds as well, and when the free
    /// trees are all cleared and the trees are still over the limit, the idle leased trees
    /// holding the most bytes of their own are cleared whole (`disk/leased.rs`).
    pub fn hold_disk(&self, short: u64) -> Result<DiskReport> {
        let mut report = DiskReport::default();
        let mut objects = objects::Swept::default();
        self.each_idle_tree(|dir| objects += objects::sweep_objects(dir))?;
        report.objects = objects.files;
        report.object_bytes = objects.bytes;
        let before = self.measure().total();
        let room = (short > 0).then(|| before.saturating_sub(short));
        let limit = match (self.inner.config.disk_limit, room) {
            (Some(limit), Some(room)) => Some(limit.min(room)),
            (limit, room) => limit.or(room),
        };
        report.before = before;
        report.after = before;
        report.limit = limit;
        let Some(limit) = limit else {
            return Ok(report);
        };
        if before <= limit {
            return Ok(report);
        }
        let mut superseded = Vec::new();
        self.each_idle_tree(|dir| superseded.extend(sweep_superseded(dir)))?;
        report.superseded = superseded;
        let mut space = self.measure();
        report.after = space.total();
        let free: HashSet<TreeId> = self.free_rows()?.iter().map(id_of).collect::<Result<_>>()?;
        for own_only in [true, false] {
            for row in self.free_rows()? {
                if report.after <= limit {
                    break;
                }
                let id = id_of(&row)?;
                if !space.holds_any(&id) {
                    continue;
                }
                let freed = if own_only {
                    space.freed_with(&id, &HashSet::new())
                } else {
                    space.freed_with(&id, &free)
                };
                if freed == 0 || (own_only && freed < OWN_BYTES) {
                    continue;
                }
                let Some(mut row) = self.take_for_sweep(&row.id)? else {
                    continue;
                };
                // Forgotten before anything is deleted: the build manager would otherwise still
                // answer that the tree holds its products, and the gate would run checks against
                // binaries that are gone. A daemon restart partway through the deletion would
                // otherwise leave the record of builds whose files are half deleted.
                self.inner.builds.forget(&id);
                let path = self.inner.config.path(&id);
                for dir in self.inner.config.keep.get().iter() {
                    remove(&path.join(dir));
                }
                row.built_commit = None;
                self.put_back(&row)?;
                report.after = report.after.saturating_sub(space.clear(&id));
                report.cleared.push(id);
            }
        }
        if report.after > limit {
            self.reclaim_leased(&mut space, limit, &mut report)?;
        }
        if !report.cleared.is_empty() || !report.reclaimed.is_empty() {
            report.after = self.measure().total();
        }
        self.serve_waiters();
        Ok(report)
    }

    /// Calls `sweep` with the build directories of every free tree, marked `resetting`
    /// meanwhile, and of every leased tree that runs nothing, its builds paused meanwhile.
    fn each_idle_tree(&self, mut sweep: impl FnMut(&PathBuf)) -> Result<()> {
        for row in self.free_rows()? {
            let Some(row) = self.take_for_sweep(&row.id)? else {
                continue;
            };
            let path = self.inner.config.path(&id_of(&row)?);
            for dir in self.inner.config.keep.get().iter() {
                sweep(&path.join(dir));
            }
            self.put_back(&row)?;
        }
        self.each_idle_leased(sweep)
    }

    /// Frees disk space fast while the disk has less than `floor` bytes free, without
    /// measuring the trees first. Deletes the whole build directories of free trees, used
    /// longest ago first, and reads the disk's free space with `free` after each one, stopping
    /// once it reaches `floor` or cannot be read. With no free tree left, it deletes the stale
    /// object files of the leased trees that run nothing, and then their whole build
    /// directories, leased longest ago first (`disk/leased.rs`).
    ///
    /// [`Trees::hold_disk`] measures every tree in full twice before it clears any, which took
    /// over half an hour per measurement on 1.6 TB of build directories under load. The daemon
    /// restarted for each release every few minutes, so no round ever finished, and the disk
    /// filled to 4 GB free with every build waiting for space. A full disk needs space now; which
    /// tree holds the most bytes of its own matters less than freeing any. A tree whose files
    /// are clones of another's frees little, and the next free tree goes after it.
    pub fn free_disk(&self, floor: u64, free: impl Fn() -> Option<u64>) -> Result<DiskReport> {
        let mut report = DiskReport {
            limit: Some(floor),
            ..DiskReport::default()
        };
        // Once the disk is back above the floor, or its free space cannot be read, nothing
        // more is deleted and the free space is not read again.
        let done = std::cell::Cell::new(false);
        let low = || {
            let low = !done.get() && free().is_some_and(|bytes| bytes < floor);
            done.set(!low);
            low
        };
        for row in self.free_rows()? {
            if !low() {
                break;
            }
            let Some(mut row) = self.take_for_sweep(&row.id)? else {
                continue;
            };
            let id = id_of(&row)?;
            // Forgotten before anything is deleted, as in `hold_disk`.
            self.inner.builds.forget(&id);
            let path = self.inner.config.path(&id);
            for dir in self.inner.config.keep.get().iter() {
                remove(&path.join(dir));
            }
            row.built_commit = None;
            self.put_back(&row)?;
            report.cleared.push(id);
        }
        if low() {
            let mut objects = objects::Swept::default();
            self.each_idle_leased(|dir| objects += objects::sweep_objects(dir))?;
            report.objects = objects.files;
            report.object_bytes = objects.bytes;
        }
        for row in self.leased_rows()? {
            if !low() {
                break;
            }
            let id = id_of(&row)?;
            if self.clear_idle_leased(&id)? {
                report.reclaimed.push(id);
            }
        }
        self.serve_waiters();
        Ok(report)
    }

    /// The free trees, used longest ago first; never used counts as longest ago.
    fn free_rows(&self) -> Result<Vec<rows::Tree>> {
        let mut free: Vec<rows::Tree> = store::all(&self.lock().db)
            .map_err(|e| store_error(&e))?
            .into_iter()
            .filter(|row| row.state == store::State::Free.as_str())
            .collect();
        free.sort_by(|a, b| a.last_used.cmp(&b.last_used).then(a.id.cmp(&b.id)));
        Ok(free)
    }

    /// Marks a free tree `resetting` for a sweep. Returns its row as it was when it was taken,
    /// or `None` if a lease took it first. The row is read in the same lock as the change of
    /// state, so a lease and release since the caller listed the free trees cannot leave it
    /// writing back a stale `built_commit` or `last_used`.
    pub(super) fn take_for_sweep(&self, id: &str) -> Result<Option<rows::Tree>> {
        let locked = self.lock();
        let Some(row) = store::get(&locked.db, id).map_err(|e| store_error(&e))? else {
            return Ok(None);
        };
        if row.state != store::State::Free.as_str() {
            return Ok(None);
        }
        let taken = store::lease(
            &locked.db,
            id,
            store::State::Resetting,
            None,
            None,
            None,
            &self.now(),
        )
        .map_err(|e| store_error(&e))?;
        Ok(taken.then_some(row))
    }

    /// Writes a swept tree's row back, which frees it. If that write fails, tries once more to
    /// free the tree by changing only its state, so a store error cannot leave it `resetting`
    /// (nothing but a daemon restart would move it).
    pub(super) fn put_back(&self, row: &rows::Tree) -> Result<()> {
        let locked = self.lock();
        if let Err(error) = store::upsert(&locked.db, row) {
            let _ = store::transition(
                &locked.db,
                &row.id,
                store::State::Resetting,
                store::State::Free,
                &self.now(),
            );
            return Err(store_error(&error));
        }
        Ok(())
    }
}

pub(super) fn id_of(row: &rows::Tree) -> Result<TreeId> {
    TreeId::new(row.id.clone()).map_err(|e| Error::Invalid(e.to_string()))
}

fn remove(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

/// Deletes the replaced builds in a Cargo target directory and returns the fingerprint
/// folders of the units deleted. A directory that is not a target directory loses nothing.
///
/// A target directory has profile directories (`<target>/<profile>`, or
/// `<target>/<triple>/<profile>` for a cross build), each with a fingerprint folder per unit.
/// A unit that no other unit depends on is a top-level build, such as a test binary or a
/// program. Top-level units of the same crate, kind and settings are one build made against
/// other dependencies, and of those only the one written last is kept. Every unit that a kept
/// top-level unit depends on, directly or through others, is kept too, and the rest are deleted.
///
/// Comparing only units of the same crate and settings, at every level, deleted builds that
/// were still in use. A unit that runs a build script records 0 for its profile, target and
/// features, so a crate's build-script runs for the `dev` and `test` profiles looked like one
/// unit built twice and one of them was deleted; the next build of that profile then rebuilt the
/// crate and everything above it. Two builds of one library with other features in their
/// dependencies, which two commands resolving features differently both use, went the same way.
pub(super) fn sweep_superseded(target: &Path) -> Vec<PathBuf> {
    let mut deleted = Vec::new();
    for profile in profile_dirs(target) {
        let Ok(entries) = fs::read_dir(profile.join(FINGERPRINTS)) else {
            continue;
        };
        let units: Vec<Unit> = entries
            .filter_map(io::Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| Unit::read(&entry.path(), &entry.file_name().to_string_lossy()))
            .collect();
        for unit in replaced(&units) {
            // The fingerprint folder goes first, and the unit's files only once it is gone. A
            // sweep stopped partway, or a folder that cannot be deleted, then leaves either the
            // whole unit or files that no fingerprint names, which Cargo never reads and
            // overwrites if it builds that unit again. The other order could leave a fingerprint
            // whose outputs were gone, which Cargo reports as `FailedToReadMetadata`.
            let folder = profile.join(FINGERPRINTS).join(&unit.name);
            remove(&folder);
            if fs::symlink_metadata(&folder).is_ok() {
                continue;
            }
            for files_folder in UNIT_FOLDERS {
                let Ok(files) = fs::read_dir(profile.join(files_folder)) else {
                    continue;
                };
                for file in files.filter_map(io::Result::ok) {
                    if belongs(&file.file_name().to_string_lossy(), &unit.name) {
                        remove(&file.path());
                    }
                }
            }
            deleted.push(folder);
        }
    }
    deleted
}

/// One unit of a profile directory, as its fingerprint folder describes it.
#[derive(Debug)]
struct Unit {
    /// The folder's name, `<crate>-<hash>`.
    name: String,
    /// What it builds apart from its dependencies ([`unit_kind`]).
    kind: String,
    written: SystemTime,
    /// The fingerprints Cargo recorded for it, as its dependents record them.
    fingerprints: Vec<u64>,
    /// Its dependencies: each one's name as the unit refers to it and its fingerprint.
    deps: Vec<(String, u64)>,
}

impl Unit {
    fn read(folder: &Path, name: &str) -> Unit {
        let mut fingerprints = Vec::new();
        let mut deps = Vec::new();
        if let Ok(files) = fs::read_dir(folder) {
            for file in files.filter_map(io::Result::ok) {
                let file_name = file.file_name().to_string_lossy().into_owned();
                if Path::new(&file_name)
                    .extension()
                    .is_some_and(|e| e == "json")
                {
                    deps = read_deps(&file.path());
                } else if !skipped(&file_name) {
                    // Cargo writes a fingerprint as the hex of its little-endian bytes and
                    // records it in a dependent's `deps` as the number itself.
                    let text = fs::read_to_string(file.path()).unwrap_or_default();
                    if let Ok(value) = u64::from_str_radix(text.trim(), 16) {
                        fingerprints.push(value.swap_bytes());
                    }
                }
            }
        }
        Unit {
            name: name.to_owned(),
            kind: unit_kind(folder, name),
            written: newest_write(folder),
            fingerprints,
            deps,
        }
    }

    /// The crate's name with `_` for `-`, as a dependent names it.
    fn crate_name(&self) -> String {
        self.name
            .rsplit_once('-')
            .map_or(self.name.as_str(), |(c, _)| c)
            .replace('-', "_")
    }
}

/// The `deps` of a fingerprint's JSON: entries of `[package hash, name, public, fingerprint]`.
fn read_deps(path: &Path) -> Vec<(String, u64)> {
    let record: serde_json::Value = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    record
        .get("deps")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|dep| {
            let dep = dep.as_array()?;
            Some((dep.get(1)?.as_str()?.to_owned(), dep.get(3)?.as_u64()?))
        })
        .collect()
}

/// The units of one profile directory that no kept build uses ([`sweep_superseded`]).
///
/// A dependency whose fingerprint matches no unit (its unit was rebuilt in place since its
/// dependent was built) keeps every unit of that crate, since which one the dependent will use
/// cannot be told. A build script's own build is named `build_script_build` by the unit that
/// runs it, so that name stands for the dependent's own crate.
fn replaced(units: &[Unit]) -> Vec<&Unit> {
    let by_fingerprint: HashMap<u64, usize> = units
        .iter()
        .enumerate()
        .flat_map(|(i, unit)| unit.fingerprints.iter().map(move |&f| (f, i)))
        .collect();
    let mut depended = vec![false; units.len()];
    let mut unmatched: HashSet<String> = HashSet::new();
    for unit in units {
        for (name, fingerprint) in &unit.deps {
            match by_fingerprint.get(fingerprint) {
                Some(&i) => depended[i] = true,
                None if name == "build_script_build" => {
                    unmatched.insert(unit.crate_name());
                }
                None => {
                    unmatched.insert(name.clone());
                }
            }
        }
    }
    let mut newest: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, unit) in units.iter().enumerate() {
        if depended[i] {
            continue;
        }
        let best = newest.entry(unit.kind.as_str()).or_insert(i);
        if (units[*best].written, &units[*best].name) < (unit.written, &unit.name) {
            *best = i;
        }
    }
    let mut kept = vec![false; units.len()];
    let mut stack: Vec<usize> = newest.into_values().collect();
    stack.extend((0..units.len()).filter(|&i| unmatched.contains(&units[i].crate_name())));
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut kept[i], true) {
            continue;
        }
        stack.extend(
            units[i]
                .deps
                .iter()
                .filter_map(|(_, fingerprint)| by_fingerprint.get(fingerprint).copied()),
        );
    }
    let mut gone: Vec<&Unit> = (0..units.len())
        .filter(|&i| !kept[i])
        .map(|i| &units[i])
        .collect();
    gone.sort_by(|a, b| a.name.cmp(&b.name));
    gone
}

/// Files of a fingerprint folder that are neither its JSON nor a fingerprint.
fn skipped(file_name: &str) -> bool {
    ["dep-", "output-", "invoked."]
        .iter()
        .any(|prefix| file_name.starts_with(prefix))
}

/// The directories one or two levels under `target` that hold a fingerprint folder.
pub(super) fn profile_dirs(target: &Path) -> Vec<PathBuf> {
    let children = |dir: &Path| -> Vec<PathBuf> {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(io::Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut found = Vec::new();
    for child in children(target) {
        if child.join(FINGERPRINTS).is_dir() {
            found.push(child);
            continue;
        }
        found.extend(
            children(&child)
                .into_iter()
                .filter(|grandchild| grandchild.join(FINGERPRINTS).is_dir()),
        );
    }
    found.sort();
    found
}

/// What a unit builds apart from its dependencies: its crate, the names of its fingerprint
/// files (such as `lib-serde` or `test-lib-serde`) and the [`UNIT_FIELDS`] of its JSON.
fn unit_kind(folder: &Path, name: &str) -> String {
    let crate_name = name.rsplit_once('-').map_or(name, |(c, _)| c);
    let mut kinds = Vec::new();
    let mut fields = Vec::new();
    if let Ok(files) = fs::read_dir(folder) {
        for file in files.filter_map(io::Result::ok) {
            let file_name = file.file_name().to_string_lossy().into_owned();
            if Path::new(&file_name)
                .extension()
                .is_some_and(|e| e == "json")
            {
                let record: serde_json::Value = fs::read_to_string(file.path())
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or_default();
                fields = UNIT_FIELDS
                    .iter()
                    .map(|key| record.get(key).map(ToString::to_string).unwrap_or_default())
                    .collect();
            } else if !skipped(&file_name) {
                kinds.push(file_name);
            }
        }
    }
    kinds.sort();
    format!("{crate_name}\0{}\0{}", kinds.join(","), fields.join(","))
}

/// Whether a file in a unit folder belongs to `unit` (`<crate>-<hash>`): `libfoo-1a2b.rlib`,
/// `foo-1a2b.d` and `foo-1a2b` all belong to `foo-1a2b`.
fn belongs(file: &str, unit: &str) -> bool {
    let stem = file.split('.').next().unwrap_or(file);
    stem == unit || stem.strip_prefix("lib") == Some(unit)
}

/// The latest modification time among a folder's entries.
fn newest_write(folder: &Path) -> SystemTime {
    let Ok(entries) = fs::read_dir(folder) else {
        return SystemTime::UNIX_EPOCH;
    };
    entries
        .filter_map(io::Result::ok)
        .filter_map(|entry| entry.metadata().and_then(|m| m.modified()).ok())
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH)
}
