//! Holding the trees under the disk budget, using the volume's available space and the
//! fleet cap to decide how much they may hold.
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
//! sweep. Idle leased trees are swept for obsolete files. Their current builds stay until
//! release unless the filesystem is below its emergency floor (`disk/floor.rs`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignite_core::capacity::{BYTES_PER_GB, disk_budget_bytes};
use ignite_core::ids::TreeId;
use ignite_core::protocol::machine_state::ReclaimableDisk;
use ignite_core::traits::{Error, Result};
use ignite_os::system::{DiskSpace, disk_space};
use ignite_store::local::{rows, trees as store};
use serde::Serialize;

use super::{DiskLimitSource, Kind, TreeConfig, Trees, store_error};
pub use floor::{BusyTree, FloorStuck};
use space::{Space, Stop};

mod floor;
mod leased;
mod objects;
mod space;
#[cfg(test)]
mod tests;
pub(crate) mod usage;

/// The bytes a free tree's deletion must free alone for the first pass of [`Trees::hold_disk`]
/// to clear it. A tree whose files are all clones of another tree's still writes its own lock
/// files and Cargo's small records, a few megabytes; a tree that built a ticket's change holds
/// gigabytes of its own.
const OWN_BYTES: u64 = 1_000_000_000;

/// The folder of per-unit fingerprints in each profile directory of a Cargo target directory,
/// which is how such a directory is recognised among a product's outputs.
pub(super) const FINGERPRINTS: &str = ".fingerprint";
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
    pub limit_source: Option<DiskLimitSource>,
    /// Replaced builds deleted, as unit folder paths.
    pub superseded: Vec<PathBuf>,
    /// Free trees whose build directories were deleted whole.
    pub cleared: Vec<TreeId>,
    /// Leased trees, idle at the time, whose incremental caches were deleted.
    pub incremental: Vec<TreeId>,
    /// Leased trees, idle at the time, whose build directories were deleted whole.
    pub reclaimed: Vec<TreeId>,
    /// Object files that newer compiles of the same codegen unit replaced, deleted before the
    /// trees were measured, and the bytes they held (`disk/objects.rs`).
    pub objects: u64,
    pub object_bytes: u64,
    /// The measurement stopped partway because the disk fell below its floor of free space, so
    /// that the disk loop could clear builds at once ([`Trees::hold_disk`]).
    pub interrupted: bool,
    /// Set when [`Trees::free_disk`] left the disk below its floor with nothing more it could
    /// delete safely: what holds the space, for the disk loop to report.
    pub stuck: Option<FloorStuck>,
}

impl TreeConfig {
    /// The same budget is used by the disk loop and doctor. The volume includes every block
    /// it holds only once, so subtract the clone-aware tree total to find the bytes outside.
    pub(crate) fn disk_budget(
        disk: Option<DiskSpace>,
        used: Option<u64>,
        cap_gb: Option<u32>,
    ) -> Option<(u64, DiskLimitSource)> {
        let cap = cap_gb.map(|gb| u64::from(gb) * BYTES_PER_GB);
        let derived = disk.zip(used).map(|(disk, used)| {
            let outside = disk
                .total
                .saturating_sub(disk.available)
                .saturating_sub(used);
            disk_budget_bytes(disk.total, outside)
        });
        match (derived, cap) {
            (Some(derived), Some(cap)) if cap < derived => Some((cap, DiskLimitSource::Cap)),
            (Some(derived), _) => Some((derived, DiskLimitSource::Disk)),
            (None, cap) => cap.map(|cap| (cap, DiskLimitSource::Cap)),
        }
    }
}

impl Trees {
    /// The bytes that deleting each known tree's build directories alone would free: the
    /// bytes they share with clones in another tree are not counted ([`Space`]).
    pub fn disk_usage(&self) -> Vec<(TreeId, u64)> {
        self.measure().own()
    }

    /// Measures the build directories of all managed trees with shared blocks counted once.
    fn measure(&self) -> Space {
        self.measure_until(&Stop::new(&|| false))
            .unwrap_or_default()
    }

    /// [`Trees::measure`], or `None` once `stop` says to stop partway.
    fn measure_until(&self, stop: &Stop) -> Option<Space> {
        let config = &self.inner.config;
        let keep = config.keep.get();
        let rows = store::all(&self.lock().db).unwrap_or_default();
        let trees = rows.iter().filter_map(|row| id_of(row).ok()).map(|id| {
            let path = config.path(&id);
            let dirs = keep.iter().map(|dir| path.join(dir)).collect();
            (id, dirs)
        });
        Space::measure_until(trees, stop)
    }

    /// Reuse a completed walk for both the total disk cap and machine history.
    fn measure_usage(&self, stop: &Stop) -> Option<(Space, u64)> {
        let space = self.measure_until(stop)?;
        let rows = store::all(&self.lock().db).ok();
        let tickets_in = |state: store::State| {
            rows.iter()
                .flatten()
                .filter(|row| row.kind == Kind::Ticket.as_str() && row.state == state.as_str())
                .filter_map(|row| id_of(row).ok())
                .collect()
        };
        let (total, bytes) = space.totals(&tickets_in(store::State::Free));
        let reclaimable = rows.as_ref().map(|_| ReclaimableDisk {
            bytes,
            measured_at: self.inner.clock.now(),
        });
        let leased = rows
            .as_ref()
            .map(|_| space.bytes_held_by(&tickets_in(store::State::Leased)));
        usage::record(&self.inner.config.dir, total, reclaimable, leased);
        Some((space, total))
    }

    pub(super) fn refresh_disk_usage(&self, stop: &dyn Fn() -> bool) {
        let _ = self.measure_usage(&Stop::new(stop));
    }

    pub(crate) fn reclaimable_disk(&self) -> Option<ReclaimableDisk> {
        usage::reclaimable(self)
    }

    /// The last fleet notice, from the same local database the heartbeat writes to.
    pub(crate) fn fleet_notice(&self) -> Option<ignite_core::fleet::FleetNotice> {
        crate::fleet_notice::read(&self.lock().db)
    }

    pub(crate) fn disk_cap_gb(&self) -> Option<u32> {
        self.fleet_notice().and_then(|notice| notice.disk_cap_gb)
    }

    /// Reclaims unused outputs when the trees exceed the volume's current budget.
    ///
    /// Free trees are cleared in two passes, each in the order of last use. The first clears
    /// only trees whose deletion alone frees at least [`OWN_BYTES`]. A tree seeded from another
    /// holds clones of the donor's files, and deleting either frees almost nothing while the
    /// other keeps the shared blocks, so a donor is kept while a tree that holds bytes of its
    /// own can go instead. The second pass clears any free tree whose bytes would be freed if
    /// every free tree went, which takes the clones that only free trees share.
    ///
    /// Leased trees that run nothing are swept of replaced builds as well. Keep their usable
    /// outputs and incremental caches between commands so an oversized working set can
    /// drain instead of repeatedly rebuilding. Handoffs account for the budget separately.
    ///
    /// `stop` is asked before each directory the sweep reads, and once it says yes the sweep
    /// returns at once with [`DiskReport::interrupted`] set. The disk loop asks whether the disk
    /// fell below its floor: a measurement of hundreds of gigabytes takes many minutes, and the
    /// disk can fill meanwhile, which needs builds cleared now and not once the walk ends.
    pub fn hold_disk(&self, stop: &dyn Fn() -> bool) -> Result<DiskReport> {
        let dir = &self.inner.config.dir;
        let budget =
            |used| TreeConfig::disk_budget(disk_space(dir).ok(), Some(used), self.disk_cap_gb());
        self.hold_disk_until(budget, stop)
    }

    #[cfg(test)]
    pub(super) fn hold_disk_with(
        &self,
        disk: impl FnOnce() -> Option<DiskSpace>,
    ) -> Result<DiskReport> {
        self.hold_disk_under(|used| TreeConfig::disk_budget(disk(), Some(used), self.disk_cap_gb()))
    }

    #[cfg(test)]
    pub(super) fn hold_disk_under(
        &self,
        budget: impl FnOnce(u64) -> Option<(u64, DiskLimitSource)>,
    ) -> Result<DiskReport> {
        self.hold_disk_until(budget, &|| false)
    }

    pub(super) fn hold_disk_until(
        &self,
        budget: impl FnOnce(u64) -> Option<(u64, DiskLimitSource)>,
        stop: &dyn Fn() -> bool,
    ) -> Result<DiskReport> {
        let stop = &Stop::new(stop);
        let mut report = DiskReport::default();
        let mut objects = objects::Swept::default();
        self.each_idle_tree(|dir| {
            if !stop.now() {
                objects += objects::sweep_objects(dir);
            }
        })?;
        report.objects = objects.files;
        report.object_bytes = objects.bytes;
        let Some((_, before)) = self.measure_usage(stop) else {
            return Ok(interrupted(report));
        };
        report.before = before;
        report.after = before;
        // Read free space after sweeping stale objects so their bytes are excluded on both sides.
        let Some((limit, source)) = budget(before) else {
            return Ok(report);
        };
        report.limit = Some(limit);
        report.limit_source = Some(source);
        if before <= limit {
            return Ok(report);
        }
        let mut superseded = Vec::new();
        self.each_idle_tree(|dir| {
            if !stop.now() {
                superseded.extend(sweep_superseded(dir));
            }
        })?;
        report.superseded = superseded;
        let Some((mut space, after)) = self.measure_usage(stop) else {
            return Ok(interrupted(report));
        };
        report.after = after;
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
                if !self.clear_free(&id)? {
                    continue;
                }
                report.after = report.after.saturating_sub(space.clear(&id));
                report.cleared.push(id);
            }
        }
        if !report.cleared.is_empty() {
            let Some((_, measured)) = self.measure_usage(stop) else {
                return Ok(interrupted(report));
            };
            report.after = measured;
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

    /// Make room for a handoff by clearing free ticket trees in order of last use. Trees
    /// with private blocks go first, so a clone donor is kept when its deletion would
    /// recover little. Only measure all the trees if shared blocks still need looking at.
    ///
    /// The measurement of shared blocks gives way when `stop` says yes, as in
    /// [`Trees::hold_disk`]: this is a best-effort cleanup, and the disk's floor comes first.
    pub(super) fn free_unused_disk(
        &self,
        target: u64,
        free: impl Fn() -> Option<u64>,
        stop: &dyn Fn() -> bool,
    ) -> Result<DiskReport> {
        let mut report = DiskReport {
            limit: Some(target),
            ..DiskReport::default()
        };
        let mut short = true;
        let mut needs_space = || {
            short = short && free().is_some_and(|free| free < target);
            short
        };
        let config = &self.inner.config;
        let keep = config.keep.get();
        let mut shared = Vec::new();
        for row in self.free_rows()? {
            if !needs_space() {
                break;
            }
            let id = id_of(&row)?;
            let path = config.path(&id);
            if row.kind != Kind::Ticket.as_str() || !keep.iter().any(|dir| path.join(dir).exists())
            {
                continue;
            }
            if !Space::private_enough(keep.iter().map(|dir| path.join(dir)), OWN_BYTES) {
                shared.push(id);
            } else if needs_space() && self.clear_free(&id)? {
                report.cleared.push(id);
            }
        }
        if !shared.is_empty()
            && needs_space()
            && let Some(mut space) = self.measure_until(&Stop::new(stop))
        {
            let candidates: HashSet<_> = shared.iter().cloned().collect();
            for id in shared {
                if !needs_space() {
                    break;
                }
                if space.group_can_free(&id, &candidates, OWN_BYTES) && self.clear_free(&id)? {
                    space.clear(&id);
                    report.cleared.push(id);
                }
            }
        }
        self.serve_waiters();
        Ok(report)
    }

    /// Forget the build before touching its files so a restart cannot leave records of
    /// missing outputs. Marking the tree keeps leases and seeding out during the deletion.
    fn clear_free(&self, id: &TreeId) -> Result<bool> {
        let Some(mut row) = self.take_for_sweep(id.as_str())? else {
            return Ok(false);
        };
        self.inner.builds.forget(id);
        let path = self.inner.config.path(id);
        for dir in self.inner.config.keep.get().iter() {
            remove(&path.join(dir));
        }
        row.built_commit = None;
        self.put_back(&row)?;
        Ok(true)
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

/// A sweep's report once it stopped for the disk's floor; what it deleted so far stays listed.
fn interrupted(report: DiskReport) -> DiskReport {
    DiskReport {
        interrupted: true,
        ..report
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
