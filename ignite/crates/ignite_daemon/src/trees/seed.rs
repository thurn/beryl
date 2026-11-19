//! Seeding a newly leased tree's Cargo build directories from another tree's.
//!
//! Each tree builds in its own directories, so when an epic started eight tickets at once,
//! eight trees each built the same products from nothing: on 2026-11-18 one product (about
//! 950 CPU-s) was built in seven trees at once and another (about 1,400 CPU-s) in six. A tree
//! is leased at the tip, and another tree usually holds a build near it.
//!
//! So a lease for a compiling ticket first copies the build of the tree nearest the tip, when
//! that is nearer than the leased tree's own. The copy runs after the lease has returned, on the
//! thread that then prebuilds the tree ([`Trees::start_builds`]): a copy of large target
//! directories under load took up to 141 s, and the session could do nothing while `ignite
//! start` waited for it. Every build in the tree waits for the copy ([`wait_until_seeded`]),
//! and a file the session edits during the copy keeps its own modification time. On macOS the copy is a clone (APFS
//! `clonefile`): it takes a few seconds and almost no disk until either tree writes. Measured
//! on 2026-11-18 with the two trees 14 commits apart (none of them touching a crate), three
//! products whose cold builds took 947, 1,070 and 1,390 CPU-s then built in 5.4, 6.3 and
//! 0.2 CPU-s.
//!
//! Cargo decides whether a unit is current from the modification times of the files it read,
//! and from a few absolute paths. Three things make the copy look to Cargo exactly as the
//! donor's build looks to it in the donor:
//!
//! - Every file git lists in the leased tree whose content is the donor's takes the donor
//!   file's modification time, and every other file (changed between the two commits, or
//!   uncommitted in either tree) is touched now, so the units that read it are rebuilt. A
//!   build script that declares no `rerun-if-changed` records its package's newest
//!   modification time itself, so without this step it would run again in every copy.
//! - Absolute paths into the donor in Cargo's dependency files (`.fingerprint/*/dep-*`, where
//!   an `include_str!` of a file outside the package is recorded with its full path) and in
//!   build scripts' recorded output (`build/*/output` and `root-output`) are rewritten to the
//!   leased tree, as is the donor's git directory, which a build script watching the commit
//!   names.
//! - Every Cargo target directory of the leased tree is replaced, not merged: a unit of its own
//!   left beside the copy could have been built from content older than the time a file now
//!   carries, and would be taken as current.
//!
//! A donor is a free tree, which is marked `resetting` for the copy so that nothing leases,
//! sweeps or prebuilds it meanwhile, or a leased ticket tree. Either way the copy holds Cargo's
//! own lock files in each profile directory, shared, so no build writes the donor while it is
//! copied but two leases may copy it at once; a donor a build holds is passed over. A tree
//! being seeded is never a donor, since its copied lock files are held by nobody. Directories a product writes that are not Cargo
//! target directories (`node_modules`, a Python environment) are left alone: they are current
//! by their input hash, not by modification times.
//!
//! Elsewhere than macOS a copy would write every byte again, so nothing is seeded there.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_git::{changed, refs, tracked};
use ignite_store::local::{rows, trees as store};

use super::disk::{id_of, profile_dirs};
use super::{Kind, LeaseRequest, Trees, store_error};

/// The lock files Cargo holds in a profile directory while it builds there: Cargo 1.98 holds the
/// build and artifact locks exclusively for the whole build, and older versions `.cargo-lock`.
const CARGO_LOCKS: [&str; 3] = [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"];

/// The trees being seeded now.
static SEEDING: Mutex<Vec<Marker>> = Mutex::new(Vec::new());

/// Signalled whenever a tree's seed ends, for builds waiting on it.
static SEEDED: Condvar = Condvar::new();

/// How often a build waiting on a seed checks whether its work was asked to stop.
const STOP_RECHECK: Duration = Duration::from_secs(1);

/// A tree being seeded: its path, its ID, and the donor once one is chosen.
struct Marker {
    path: PathBuf,
    tree: TreeId,
    donor: Option<TreeId>,
}

fn seeding() -> MutexGuard<'static, Vec<Marker>> {
    SEEDING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks a tree as being seeded until it is dropped. While the mark is held, every build in the
/// tree waits ([`wait_until_seeded`]), the tree is never a donor, and a release of the tree waits
/// before it resets it.
pub struct Seeding(PathBuf);

impl Seeding {
    /// Marks the tree `tree` at `path`.
    pub fn start(path: PathBuf, tree: TreeId) -> Seeding {
        seeding().push(Marker {
            path: path.clone(),
            tree,
            donor: None,
        });
        Seeding(path)
    }

    /// Records the tree the copy comes from, for the message a waiting build shows.
    fn set_donor(&self, donor: &TreeId) {
        if let Some(marker) = seeding().iter_mut().find(|m| m.path == self.0) {
            marker.donor = Some(donor.clone());
        }
    }

    fn contains(path: &Path) -> bool {
        seeding().iter().any(|m| m.path == path)
    }
}

impl Drop for Seeding {
    fn drop(&mut self) {
        let mut seeding = seeding();
        if let Some(at) = seeding.iter().position(|m| m.path == self.0) {
            seeding.remove(at);
        }
        SEEDED.notify_all();
    }
}

/// Returns once the tree at `path` is not being seeded, which is at once for every tree but one
/// whose lease has just copied another tree's build. The seed deletes and replaces the tree's
/// Cargo target directories, so a build that ran meanwhile would be deleted or mixed with the
/// copy. While it waits, the calling work is shown as waiting for the copy. `Err` says why it
/// stopped waiting when the work was asked to stop first.
pub fn wait_until_seeded(path: &Path) -> std::result::Result<(), String> {
    let mut seeding = seeding();
    let mut waiting = None;
    while let Some(marker) = seeding.iter().find(|m| m.path == path) {
        let what = match &marker.donor {
            Some(donor) => format!(
                "{}'s build directories to be copied from {donor}",
                marker.tree
            ),
            None => format!("{}'s build directories to be copied", marker.tree),
        };
        if crate::steps::stop_requested() {
            return Err(format!(
                "stopped waiting for {what}: its work was asked to stop"
            ));
        }
        if waiting.as_ref().is_none_or(|(shown, _)| *shown != what) {
            waiting = Some((what.clone(), crate::steps::waiting(&what)));
        }
        seeding = SEEDED
            .wait_timeout(seeding, STOP_RECHECK)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    Ok(())
}

/// The tree a build was copied from.
#[derive(Debug, PartialEq, Eq)]
pub struct Seeded {
    pub donor: TreeId,
    /// The Cargo target directories copied, relative to the tree's root.
    pub dirs: Vec<String>,
}

/// A donor taken for the copy, and whether it was free and must be put back.
struct Donor {
    row: rows::Tree,
    free: bool,
}

impl Trees {
    /// Starts a compiling lease's work on a thread of its own, so that the lease returns as soon
    /// as the branch is checked out: first the seed (on macOS), then the prebuild products. The
    /// tree is marked as being seeded before this returns, so a build asked for at once, by the
    /// prebuild or by the session, waits for the copy instead of running under it.
    pub(super) fn start_builds(&self, tree: &TreeId, request: &LeaseRequest) {
        let seeding = cfg!(target_os = "macos")
            .then(|| Seeding::start(self.inner.config.path(tree), tree.clone()));
        if seeding.is_none() && self.inner.config.prebuild.get().is_empty() {
            return;
        }
        let trees = self.clone();
        let (tree, at, tip) = (tree.clone(), request.at.clone(), request.tip.clone());
        std::thread::spawn(move || {
            if let Some(seeding) = seeding {
                let started = trees.inner.clock.now();
                let seeded = trees.seed(&seeding, &tree, &tip);
                drop(seeding);
                let took = trees.inner.clock.now().seconds_since(started);
                match seeded {
                    Ok(Some(seeded)) => eprintln!(
                        "ignite: {tree} starts from a copy of {}'s {}, made in {took:.1} s",
                        seeded.donor,
                        seeded.dirs.join(", ")
                    ),
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("ignite: {tree} was not seeded from another tree: {error}");
                    }
                }
            }
            if !trees.inner.config.prebuild.get().is_empty() {
                trees.build(&tree, &at);
            }
        });
    }

    /// Copies the build of the tree nearest `tip` into `tree`, if one is nearer than `tree`'s
    /// own. Returns what was copied, or `None` when nothing was.
    /// `seeding` marks `tree` for as long as the copy runs.
    fn seed(&self, seeding: &Seeding, tree: &TreeId, tip: &Commit) -> Result<Option<Seeded>> {
        let Some(donor) = self.take_donor(tree, tip)? else {
            return Ok(None);
        };
        seeding.set_donor(&id_of(&donor.row)?);
        let result = self.copy_from(&donor.row, tree);
        if donor.free {
            self.put_back(&donor.row)?;
        }
        let dirs = result?;
        if dirs.is_empty() {
            return Ok(None);
        }
        if let Some(built) = donor.row.built_commit.as_deref() {
            let built = Commit::new(built).map_err(|e| Error::Invalid(e.to_string()))?;
            self.record_build(tree, &built)?;
        }
        Ok(Some(Seeded {
            donor: id_of(&donor.row)?,
            dirs,
        }))
    }

    /// The tree to copy from: of the free trees and the leased ticket trees, the one whose last
    /// build is nearest `tip` and nearer than `tree`'s, holding a Cargo target directory. A free
    /// donor is marked `resetting` until [`Trees::put_back`].
    fn take_donor(&self, tree: &TreeId, tip: &Commit) -> Result<Option<Donor>> {
        let rows = store::all(&self.lock().db).map_err(|e| store_error(&e))?;
        let configured = self.inner.config.configured();
        let own = rows
            .iter()
            .find(|row| row.id == tree.as_str())
            .and_then(|row| self.distance(row.built_commit.as_deref(), tip));
        let mut candidates: Vec<(usize, bool, &rows::Tree)> = rows
            .iter()
            .filter(|row| row.id != tree.as_str())
            .filter_map(|row| {
                let kind = configured
                    .iter()
                    .find(|(id, _)| id.as_str() == row.id)
                    .map(|(_, kind)| *kind)?;
                let free = row.state == store::State::Free.as_str();
                let leased = row.state == store::State::Leased.as_str() && kind == Kind::Ticket;
                if !free && !leased {
                    return None;
                }
                let distance = self.distance(row.built_commit.as_deref(), tip)?;
                if own.is_some_and(|own| distance >= own) {
                    return None;
                }
                let path = self.inner.config.path(&id_of(row).ok()?);
                if Seeding::contains(&path) {
                    return None;
                }
                (!self.cargo_dirs(&path).is_empty()).then_some((distance, !free, row))
            })
            .collect();
        // Nearest first, and a free tree before a leased one at the same distance.
        candidates.sort_by(|a, b| (a.0, a.1, &a.2.id).cmp(&(b.0, b.1, &b.2.id)));
        for (_, leased, row) in candidates {
            if leased {
                return Ok(Some(Donor {
                    row: row.clone(),
                    free: false,
                }));
            }
            if let Some(row) = self.take_for_sweep(&row.id)? {
                return Ok(Some(Donor { row, free: true }));
            }
        }
        Ok(None)
    }

    /// The directories the products write under `root` that are Cargo target directories,
    /// outermost only (a target directory inside another is copied with it).
    fn cargo_dirs(&self, root: &Path) -> Vec<String> {
        let keep = self.inner.config.keep.get();
        keep.iter()
            .filter(|dir| {
                !keep
                    .iter()
                    .any(|outer| *outer != **dir && dir.starts_with(&format!("{outer}/")))
            })
            .filter(|dir| !profile_dirs(&root.join(dir)).is_empty())
            .cloned()
            .collect()
    }

    /// Replaces `tree`'s Cargo target directories with copies of the donor's and makes the
    /// tree's files look to Cargo as the donor's do. Returns the directories copied. On a
    /// failure partway, `tree`'s target directories are deleted, so that it builds from nothing
    /// rather than from a copy whose files' times were never set.
    fn copy_from(&self, donor: &rows::Tree, tree: &TreeId) -> Result<Vec<String>> {
        let from = self.inner.config.path(&id_of(donor)?);
        let to = self.inner.config.path(tree);
        let dirs = self.cargo_dirs(&from);
        let Some(_locks) = cargo_locks(&from, &dirs)? else {
            return Ok(Vec::new());
        };
        // Everything git says is read before the tree is touched.
        let pairs = [
            path_pair(&from, &to),
            path_pair(&tracked::git_dir(&from)?, &tracked::git_dir(&to)?),
        ];
        let times = file_times(&from, &to)?;
        let own = self.cargo_dirs(&to);
        // Forgotten before the tree's own target directories are deleted, and whether or not the
        // copy succeeds: a copy that fails partway leaves none, and a daemon restart during the
        // copy would otherwise leave the record of builds whose files are gone.
        self.inner.builds.forget(tree);
        let copied = (|| -> io::Result<()> {
            for dir in own.iter().chain(&dirs) {
                remove(&to.join(dir))?;
            }
            for dir in &dirs {
                clone_dir(&from.join(dir), &to.join(dir))?;
                rewrite_target(&to.join(dir), &pairs)?;
            }
            for (file, time, read) in &times {
                // The session works in the tree during the copy. A file it wrote since its time
                // was read keeps its newer time, so the units that read it are rebuilt.
                let path = to.join(file);
                if fs::metadata(&path).and_then(|m| m.modified()).ok() == Some(*read) {
                    set_modified(&path, *time)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = copied {
            for dir in own.iter().chain(&dirs) {
                let _ = remove(&to.join(dir));
            }
            return Err(error.into());
        }
        Ok(dirs)
    }
}

/// The two roots as path prefixes, each ending in `/`.
fn path_pair(from: &Path, to: &Path) -> (Vec<u8>, Vec<u8>) {
    let prefix = |path: &Path| format!("{}/", path.to_string_lossy()).into_bytes();
    (prefix(from), prefix(to))
}

/// Holds Cargo's lock files in every profile directory of `dirs` under `root`, shared: Cargo
/// holds them exclusively while it builds. `None` when a build holds one now.
fn cargo_locks(root: &Path, dirs: &[String]) -> Result<Option<Vec<File>>> {
    let mut held = Vec::new();
    for dir in dirs {
        for profile in profile_dirs(&root.join(dir)) {
            for name in CARGO_LOCKS {
                let Ok(file) = File::open(profile.join(name)) else {
                    continue;
                };
                match file.try_lock_shared() {
                    Ok(()) => held.push(file),
                    Err(fs::TryLockError::WouldBlock) => return Ok(None),
                    Err(fs::TryLockError::Error(error)) => return Err(error.into()),
                }
            }
        }
    }
    Ok(Some(held))
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Copies the directory `from` to `to`, which must not exist, keeping each file's modification
/// time. On APFS each file is a clone.
fn clone_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            clone_dir(&source, &target)?;
        } else if kind.is_symlink() {
            copy_link(&source, &target)?;
        } else {
            fs::copy(&source, &target)?;
            set_modified(&target, entry.metadata()?.modified()?)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(from)?, to)
}

#[cfg(not(unix))]
fn copy_link(_from: &Path, _to: &Path) -> io::Result<()> {
    Ok(())
}

fn set_modified(path: &Path, time: SystemTime) -> io::Result<()> {
    File::open(path)?.set_modified(time)
}

/// Rewrites the donor's paths to the tree's in the Cargo target directory `target`: in each
/// unit's dependency files and each build script's recorded output. A dependency file that
/// cannot be rewritten has its unit's fingerprint folder deleted, so the unit is rebuilt.
fn rewrite_target(target: &Path, pairs: &[(Vec<u8>, Vec<u8>)]) -> io::Result<()> {
    for profile in profile_dirs(target) {
        for unit in read_dir_paths(&profile.join(".fingerprint")) {
            for file in read_dir_paths(&unit) {
                let named = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !named.starts_with("dep-") {
                    continue;
                }
                let data = fs::read(&file)?;
                match pairs.iter().try_fold(data.clone(), |data, (old, new)| {
                    rewrite_dep_info(&data, old, new)
                }) {
                    Some(new) if new == data => {}
                    Some(new) => rewrite_file(&file, &new)?,
                    None => {
                        remove(&unit)?;
                        break;
                    }
                }
            }
        }
        for script in read_dir_paths(&profile.join("build")) {
            for name in ["output", "root-output"] {
                let file = script.join(name);
                let Ok(data) = fs::read(&file) else {
                    continue;
                };
                let new = pairs
                    .iter()
                    .fold(data.clone(), |data, (old, new)| replace(&data, old, new));
                if new != data {
                    rewrite_file(&file, &new)?;
                }
            }
        }
    }
    Ok(())
}

fn read_dir_paths(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default()
}

/// Writes `data` to `file`, keeping its modification time: Cargo compares a dependency file's
/// time with the files it lists.
fn rewrite_file(file: &Path, data: &[u8]) -> io::Result<()> {
    let modified = fs::metadata(file)?.modified()?;
    fs::write(file, data)?;
    set_modified(file, modified)
}

fn replace(data: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut rest = data;
    while let Some(at) = find(rest, old) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(new);
        rest = &rest[at + old.len()..];
    }
    out.extend_from_slice(rest);
    out
}

fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len()).position(|w| w == needle)
}

/// `data`, a Cargo dependency file, with each path that starts with `old` starting with `new`
/// instead. Cargo writes each path as its length in four little-endian bytes followed by its
/// bytes, so a path that starts with `old` starts right after its length, and the length
/// changes with the prefix. `None` when an occurrence is not where a path starts, which means
/// the format is not the one this reads.
fn rewrite_dep_info(data: &[u8], old: &[u8], new: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    let mut done = 0;
    while let Some(found) = find(&data[done..], old) {
        let at = done + found;
        let start = at.checked_sub(4)?;
        if start < done {
            return None;
        }
        let length = usize::try_from(u32::from_le_bytes(data[start..at].try_into().ok()?)).ok()?;
        if length < old.len() || at + length > data.len() {
            return None;
        }
        out.extend_from_slice(&data[done..start]);
        let new_length = u32::try_from(length - old.len() + new.len()).ok()?;
        out.extend_from_slice(&new_length.to_le_bytes());
        out.extend_from_slice(new);
        out.extend_from_slice(&data[at + old.len()..at + length]);
        done = at + length;
    }
    out.extend_from_slice(&data[done..]);
    Some(out)
}

/// The modification time each file git lists in the tree at `to` gets: that of the file at the
/// same path in `from` when their content is the same, and the present time when it is not or
/// may not be (the paths that differ between the two trees' commits, or that either tree has
/// uncommitted). Each comes with the file's own modification time when it was read.
fn file_times(from: &Path, to: &Path) -> Result<Vec<(String, SystemTime, SystemTime)>> {
    let mut differing: HashSet<String> =
        changed::between(to, &refs::head(from)?, &refs::head(to)?)?
            .into_iter()
            .collect();
    differing.extend(changed::uncommitted(from)?);
    differing.extend(changed::uncommitted(to)?);
    let now = SystemTime::now();
    let mut times = Vec::new();
    for file in tracked::files(to)? {
        let read = match fs::symlink_metadata(to.join(&file)) {
            Ok(meta) if meta.is_file() => meta.modified()?,
            _ => continue,
        };
        let time = if differing.contains(&file) {
            now
        } else {
            match fs::symlink_metadata(from.join(&file)) {
                Ok(meta) if meta.is_file() => meta.modified()?,
                _ => now,
            }
        };
        times.push((file, time, read));
    }
    Ok(times)
}

#[cfg(test)]
#[path = "seed_tests.rs"]
mod tests;
