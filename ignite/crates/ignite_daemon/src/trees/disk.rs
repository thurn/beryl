//! Holding the trees under the machine's disk limit (`trees_disk_gb` in `ignite.toml`).
//!
//! The build directories are what grows. Each change to a crate's dependencies, features or
//! profile makes Cargo build that crate, and every crate above it, under a new hash beside the
//! old one, and nothing deletes the old one. So the daemon first deletes builds that a newer
//! build of the same unit replaced, and then, if the trees are still over the limit, the whole
//! build directories of the free trees used longest ago.
//!
//! Only free trees are touched. Each is marked `resetting` while its files are deleted, so no
//! lease can take it half swept, and its row is put back as it was afterwards, so the order of
//! last use survives the sweep.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignite_core::ids::TreeId;
use ignite_core::traits::{Error, Result};
use ignite_store::local::{rows, trees as store};
use serde::Serialize;

use super::{Trees, store_error};

/// The folder of per-unit fingerprints in each profile directory of a Cargo target directory,
/// which is how such a directory is recognised among a product's outputs.
const FINGERPRINTS: &str = ".fingerprint";
/// The folders of a profile directory that hold a unit's files, named `<crate>-<hash>` with
/// optional prefixes and extensions, beside its fingerprint folder.
const UNIT_FOLDERS: [&str; 2] = ["deps", "build"];
/// The fields of a fingerprint's JSON that tell two units of one crate apart, besides their
/// dependencies: two builds that differ only in dependencies are one unit built twice.
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
}

impl Trees {
    /// Bytes on disk in each configured tree's build directories.
    pub fn disk_usage(&self) -> Vec<(TreeId, u64)> {
        let config = &self.inner.config;
        config
            .configured()
            .into_iter()
            .map(|(id, _)| {
                let path = config.path(&id);
                let bytes = config.keep.iter().map(|dir| size(&path.join(dir))).sum();
                (id, bytes)
            })
            .collect()
    }

    /// Brings the trees under the disk limit, if they are over it.
    pub fn hold_disk(&self) -> Result<DiskReport> {
        let usage = self.disk_usage();
        let before: u64 = usage.iter().map(|(_, bytes)| bytes).sum();
        let mut report = DiskReport {
            before,
            after: before,
            limit: self.inner.config.disk_limit,
            ..DiskReport::default()
        };
        let Some(limit) = report.limit else {
            return Ok(report);
        };
        if before <= limit {
            return Ok(report);
        }
        for row in self.free_rows()? {
            let Some(row) = self.take_for_sweep(row)? else {
                continue;
            };
            let path = self.inner.config.path(&id_of(&row)?);
            for dir in &self.inner.config.keep {
                report.superseded.extend(sweep_superseded(&path.join(dir)));
            }
            self.put_back(&row)?;
        }
        report.after = self.disk_usage().iter().map(|(_, bytes)| bytes).sum();
        let usage: BTreeMap<TreeId, u64> = self.disk_usage().into_iter().collect();
        for row in self.free_rows()? {
            if report.after <= limit {
                break;
            }
            let id = id_of(&row)?;
            if usage.get(&id).copied().unwrap_or(0) == 0 {
                continue;
            }
            let Some(mut row) = self.take_for_sweep(row)? else {
                continue;
            };
            let path = self.inner.config.path(&id);
            for dir in &self.inner.config.keep {
                remove(&path.join(dir));
            }
            row.built_commit = None;
            self.put_back(&row)?;
            report.after = report
                .after
                .saturating_sub(usage.get(&id).copied().unwrap_or(0));
            report.cleared.push(id);
        }
        report.after = self.disk_usage().iter().map(|(_, bytes)| bytes).sum();
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

    /// Marks a free tree `resetting` for a sweep. Returns its row as it was, or `None` if a
    /// lease took it first.
    fn take_for_sweep(&self, row: rows::Tree) -> Result<Option<rows::Tree>> {
        let locked = self.lock();
        let taken = store::lease(
            &locked.db,
            &row.id,
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
    fn put_back(&self, row: &rows::Tree) -> Result<()> {
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

fn id_of(row: &rows::Tree) -> Result<TreeId> {
    TreeId::new(row.id.clone()).map_err(|e| Error::Invalid(e.to_string()))
}

/// Bytes on disk under `path`, not following symbolic links; 0 if it does not exist.
pub(super) fn size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    let own = allocated(&meta);
    if !meta.is_dir() {
        return own;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return own;
    };
    own + entries
        .filter_map(io::Result::ok)
        .map(|entry| size(&entry.path()))
        .sum::<u64>()
}

/// The bytes a file's blocks take on disk.
#[cfg(unix)]
fn allocated(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

/// The file's length. Windows reports no block count without opening the file, so a sparse or
/// compressed file counts at its full length.
#[cfg(windows)]
fn allocated(meta: &fs::Metadata) -> u64 {
    meta.len()
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
/// Units of the same crate, kind and settings are one unit built against other dependencies;
/// of those, all but the one written last are deleted.
pub(super) fn sweep_superseded(target: &Path) -> Vec<PathBuf> {
    let mut deleted = Vec::new();
    for profile in profile_dirs(target) {
        let mut by_kind: BTreeMap<String, Vec<(SystemTime, String)>> = BTreeMap::new();
        let Ok(entries) = fs::read_dir(profile.join(FINGERPRINTS)) else {
            continue;
        };
        for entry in entries.filter_map(io::Result::ok) {
            let folder = entry.path();
            if !folder.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            by_kind
                .entry(unit_kind(&folder, &name))
                .or_default()
                .push((newest_write(&folder), name));
        }
        for mut builds in by_kind.into_values() {
            builds.sort();
            builds.pop();
            for (_, unit) in builds {
                for folder in UNIT_FOLDERS {
                    let Ok(files) = fs::read_dir(profile.join(folder)) else {
                        continue;
                    };
                    for file in files.filter_map(io::Result::ok) {
                        if belongs(&file.file_name().to_string_lossy(), &unit) {
                            remove(&file.path());
                        }
                    }
                }
                let folder = profile.join(FINGERPRINTS).join(&unit);
                remove(&folder);
                deleted.push(folder);
            }
        }
    }
    deleted
}

/// The directories one or two levels under `target` that hold a fingerprint folder.
fn profile_dirs(target: &Path) -> Vec<PathBuf> {
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
            } else if !["dep-", "output-", "invoked."]
                .iter()
                .any(|prefix| file_name.starts_with(prefix))
            {
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
