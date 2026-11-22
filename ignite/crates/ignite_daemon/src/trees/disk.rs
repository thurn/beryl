//! Cleaning one tree's build outputs before its next build.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

use ignite_core::ids::TreeId;
use ignite_core::traits::{Error, Result};
use ignite_store::local::{rows, trees as store};

use super::{Trees, store_error};

mod objects;
#[cfg(test)]
mod tests;

pub(super) const FINGERPRINTS: &str = ".fingerprint";
const UNIT_FOLDERS: [&str; 2] = ["deps", "build"];
const UNIT_FIELDS: [&str; 5] = ["features", "profile", "target", "compile_kind", "rustflags"];

impl Trees {
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
    /// (nothing but a daemon restart would move it). A lease that arrived during the sweep
    /// may be waiting for this tree, even if the sweep found no builds to delete.
    pub(super) fn put_back(&self, row: &rows::Tree) -> Result<()> {
        let locked = self.lock();
        let result = store::upsert(&locked.db, row);
        if result.is_err() {
            let _ = store::transition(
                &locked.db,
                &row.id,
                store::State::Resetting,
                store::State::Free,
                &self.now(),
            );
        }
        drop(locked);
        self.serve_waiters();
        result.map_err(|error| store_error(&error))
    }
}

pub(super) fn id_of(row: &rows::Tree) -> Result<TreeId> {
    TreeId::new(row.id.clone()).map_err(|e| Error::Invalid(e.to_string()))
}

pub(super) fn remove_output(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
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
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| Unit::read(&entry.path(), &entry.file_name().to_string_lossy()))
            .collect();
        for unit in replaced(&units) {
            if let Ok(folder) = delete_unit(&profile, &unit.name) {
                deleted.push(folder);
            }
        }
    }
    deleted
}

/// Remove the fingerprint before its outputs. If cleanup is interrupted, Cargo will rebuild
/// the unit instead of trying to read an output that still has a fingerprint but is gone.
fn delete_unit(profile: &Path, name: &str) -> io::Result<PathBuf> {
    let folder = profile.join(FINGERPRINTS).join(name);
    remove_output(&folder)?;
    for files_folder in UNIT_FOLDERS {
        let dir = profile.join(files_folder);
        if !fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir()) {
            continue;
        }
        for file in fs::read_dir(dir)? {
            let file = file?;
            if belongs(&file.file_name().to_string_lossy(), name) {
                remove_output(&file.path())?;
            }
        }
    }
    Ok(folder)
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
        if !fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir()) {
            return Vec::new();
        }
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(io::Result::ok)
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut found = Vec::new();
    for child in children(target) {
        if is_fingerprint_dir(&child) {
            found.push(child);
            continue;
        }
        found.extend(
            children(&child)
                .into_iter()
                .filter(|grandchild| is_fingerprint_dir(grandchild)),
        );
    }
    found.sort();
    found
}

fn is_fingerprint_dir(profile: &Path) -> bool {
    fs::symlink_metadata(profile.join(FINGERPRINTS)).is_ok_and(|meta| meta.is_dir())
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

mod ceiling;
pub(super) use ceiling::{Measurement, clean, directories};
