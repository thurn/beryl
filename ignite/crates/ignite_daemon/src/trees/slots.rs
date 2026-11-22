//! Count trees with compiled outputs and reserve a slot before a build or seed can write them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Weak, mpsc};
use std::thread;

use ignite_core::capacity::BYTES_PER_GB;
use ignite_core::fleet::{FleetNotice, TreeSettings, TreeSpace};
use ignite_core::ids::{TicketId, TreeId};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_os::Native;
use ignite_os::occupants::Occupants as _;
use ignite_os::system::{DiskSpace, disk_space};
use ignite_store::local::{rows, trees as store};

use super::{Inner, Locked, Seeding, Trees, disk, store_error};
use crate::supervise::Run;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BuildSlots {
    pub total: u32,
    pub free: u32,
}

impl std::fmt::Display for BuildSlots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} build slots, {} free", self.total, self.free)
    }
}

#[derive(Default)]
pub(super) struct BuildSpace {
    /// Output directories declared in a tree's own manifest can differ from the daemon's.
    outputs: BTreeMap<TreeId, BTreeSet<String>>,
    active: BTreeMap<TreeId, usize>,
    measuring: BTreeMap<TreeId, mpsc::Receiver<disk::Measurement>>,
    reclaiming: bool,
    last_cleanup: Option<(Timestamp, String)>,
    legacy_commands: Vec<(PathBuf, Run)>,
}

#[derive(Clone)]
pub(crate) struct TreeBuildControl(Weak<Inner>);

impl TreeBuildControl {
    pub(crate) fn trees(&self) -> Option<Trees> {
        self.0.upgrade().map(|inner| Trees { inner })
    }
}

/// An empty tree counts as occupied while a build or seed may create its build directories.
/// Once the last writer leaves, the directories on disk are the record of its slot.
pub(crate) struct BuildSlot {
    trees: Trees,
    tree: TreeId,
}

impl Drop for BuildSlot {
    fn drop(&mut self) {
        let mut locked = self.trees.lock();
        if let Some(count) = locked.build_space.active.get_mut(&self.tree) {
            *count -= 1;
            if *count == 0 {
                locked.build_space.active.remove(&self.tree);
            }
        }
    }
}

pub(crate) fn volume_space(root: &Path) -> Option<DiskSpace> {
    let path = root.ancestors().find(|path| path.is_dir())?;
    disk_space(path).ok()
}

pub(crate) fn ceiling_bytes(settings: &TreeSettings) -> u64 {
    u64::from(settings.tree_ceiling_gb.max(1)) * BYTES_PER_GB
}

pub(crate) fn slot_count(settings: &TreeSettings, disk: Option<DiskSpace>) -> u32 {
    let space = match settings.tree_space_gb {
        Some(TreeSpace::Gigabytes(gb)) => u64::from(gb) * BYTES_PER_GB,
        Some(TreeSpace::Automatic(_)) | None => disk.map_or(0, |disk| disk.total),
    };
    u32::try_from((space / ceiling_bytes(settings)).max(1)).unwrap_or(u32::MAX)
}

fn configured_slots(root: &Path, settings: &TreeSettings) -> u32 {
    let disk = match settings.tree_space_gb {
        Some(TreeSpace::Gigabytes(_)) => None,
        _ => volume_space(root),
    };
    slot_count(settings, disk)
}

/// The lock stays beside the checkout, so an output cleanup or a reset cannot unlink it.
/// A supervisor holds it shared until its command exits, including over daemon restarts.
pub(crate) fn command_lock(tree: &Path) -> io::Result<File> {
    let name = tree
        .file_name()
        .ok_or_else(|| io::Error::other("tree has no name"))?;
    let mut lock = std::ffi::OsString::from(".");
    lock.push(name);
    lock.push(".commands.lock");
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(tree.with_file_name(lock))
}

pub(super) fn try_exclusive(path: &Path) -> io::Result<Option<File>> {
    let lock = command_lock(path)?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

/// Tell a check that obtained its products before this cleanup to verify them again before
/// running. The exclusive lock orders each change against commands using the products.
fn invalidate_artifacts(lock: &mut File) -> io::Result<()> {
    lock.seek(SeekFrom::End(0))?;
    lock.write_all(b"x")
}

pub(crate) fn caller_processes(caller: Option<u32>) -> BTreeSet<u32> {
    let mut spared = BTreeSet::new();
    for start in [Some(std::process::id()), caller].into_iter().flatten() {
        let mut at = Some(start);
        while let Some(pid) = at.filter(|&pid| pid > 1 && spared.insert(pid)) {
            at = Native.parent(pid);
        }
    }
    spared
}

impl Trees {
    /// Commands across the first restart still run under their original supervisor.
    pub(crate) fn remember_legacy_commands(&self, runs: &[Run]) {
        self.lock().build_space.legacy_commands = runs
            .iter()
            .filter_map(|run| run.legacy_cwd().map(|path| (path, run.clone())))
            .collect();
    }

    pub(super) fn legacy_command_in(locked: &Locked, tree: &Path) -> bool {
        locked
            .build_space
            .legacy_commands
            .iter()
            .any(|(cwd, run)| cwd.starts_with(tree) && run.is_running())
    }

    pub(super) fn tree_processes_clear(&self, tree: &TreeId, path: &Path) -> io::Result<bool> {
        if !path.exists() {
            return Ok(true);
        }
        let mut spared = caller_processes(None);
        for activity in self.inner.steps.in_tree(tree, path) {
            spared.extend(activity.work().caller_processes);
        }
        if let Some(current) = crate::steps::current() {
            spared.extend(current.work().caller_processes);
        }
        Ok(Native
            .occupants(path)?
            .iter()
            .all(|process| spared.contains(&process.pid)))
    }

    pub(crate) fn build_service(&self) -> std::sync::Arc<dyn ignite_core::traits::Builds> {
        self.inner.builds.clone()
    }

    pub(crate) fn build_control(&self) -> TreeBuildControl {
        TreeBuildControl(std::sync::Arc::downgrade(&self.inner))
    }

    pub(crate) fn fleet_notice(&self) -> Option<FleetNotice> {
        crate::fleet_notice::read(&self.lock().db)
    }

    pub(crate) fn build_volume(&self) -> (PathBuf, TreeSettings) {
        let locked = self.lock();
        (
            self.root_in(&locked.db),
            crate::fleet_notice::read(&locked.db).map_or_else(TreeSettings::default, |n| n.trees),
        )
    }

    pub(crate) fn build_disk_has_room(&self) -> bool {
        let (root, settings) = self.build_volume();
        volume_space(&root).is_none_or(|disk| disk.available >= ceiling_bytes(&settings) / 2)
    }

    fn directories(&self, locked: &Locked, row: &rows::Tree) -> Vec<PathBuf> {
        let mut outputs = self.config().compiled_outputs.get().as_ref().clone();
        if let Ok(tree) = disk::id_of(row)
            && let Some(known) = locked.build_space.outputs.get(&tree)
        {
            outputs.extend(known.iter().cloned());
        }
        disk::directories(Path::new(&row.path), &outputs)
    }

    fn occupied(&self, locked: &Locked, row: &rows::Tree) -> bool {
        disk::id_of(row).is_ok_and(|id| locked.build_space.active.contains_key(&id))
            || self
                .directories(locked, row)
                .iter()
                .any(|dir| fs::symlink_metadata(dir).is_ok())
    }

    pub(crate) fn build_slots(&self) -> BuildSlots {
        self.dispatch_slots().0
    }

    pub(super) fn dispatch_slots(&self) -> (BuildSlots, BTreeSet<TicketId>) {
        let locked = self.lock();
        let root = self.root_in(&locked.db);
        let settings =
            crate::fleet_notice::read(&locked.db).map_or_else(TreeSettings::default, |n| n.trees);
        let total = configured_slots(&root, &settings);
        let mut holders = BTreeSet::new();
        let free = store::all(&locked.db).map_or(0, |rows| {
            let mut unavailable = 0_u32;
            for row in rows {
                if self.occupied(&locked, &row) && !Self::reclaimable(&locked, &row) {
                    unavailable = unavailable.saturating_add(1);
                    if let Some(ticket) = row.ticket.and_then(|id| TicketId::new(id).ok()) {
                        holders.insert(ticket);
                    }
                }
            }
            total.saturating_sub(unavailable)
        });
        (BuildSlots { total, free }, holders)
    }

    pub(super) fn build_directories(&self) -> Result<Vec<Vec<PathBuf>>> {
        let locked = self.lock();
        let rows = store::all(&locked.db).map_err(|error| store_error(&error))?;
        Ok(rows
            .iter()
            .filter(|row| self.occupied(&locked, row))
            .map(|row| self.directories(&locked, row))
            .collect())
    }

    pub(crate) fn is_compiled_output(&self, output: &Path) -> bool {
        self.config()
            .compiled_outputs
            .get()
            .iter()
            .any(|dir| Path::new(dir) == output)
    }

    pub(crate) fn create_build_output(tree: &TreeId, path: &Path, output: &Path) -> Result<()> {
        let dirs = disk::directories(
            path,
            &BTreeSet::from([output.to_string_lossy().into_owned()]),
        );
        let Some(dir) = dirs.first() else {
            return Err(Error::Invalid(format!(
                "unsafe output path in {tree}: {}",
                output.display()
            )));
        };
        if fs::symlink_metadata(dir).is_ok_and(|meta| !meta.is_dir()) {
            return Err(Error::Invalid(format!(
                "{} is not a build directory",
                dir.display()
            )));
        }
        fs::create_dir_all(dir)?;
        Ok(())
    }

    pub(crate) fn last_build_cleanup(&self) -> Option<String> {
        self.lock()
            .build_space
            .last_cleanup
            .as_ref()
            .map(|(_, text)| text.clone())
    }

    pub(crate) fn last_disk_cleanup_at(&self) -> Option<Timestamp> {
        self.lock()
            .build_space
            .last_cleanup
            .as_ref()
            .map(|&(at, _)| at)
    }

    fn reclaimable(locked: &Locked, row: &rows::Tree) -> bool {
        row.state == store::State::Free.as_str()
            && !disk::id_of(row).is_ok_and(|id| locked.build_space.active.contains_key(&id))
            && !Seeding::involves(Path::new(&row.path))
            && !Self::legacy_command_in(locked, Path::new(&row.path))
    }

    /// Reserves an existing or empty slot. A speculative seed does not evict a donor.
    /// An actual build may clear the oldest available holder. The store reserves that
    /// tree through deletion, and other requests still count its slot until deletion ends.
    pub(crate) fn take_build_slot(
        &self,
        tree: &TreeId,
        outputs: &[String],
        evict: bool,
    ) -> Result<Option<BuildSlot>> {
        let (root, settings) = self.build_volume();
        let total = configured_slots(&root, &settings);
        self.take_slot(tree, outputs, evict, total)
    }

    pub(super) fn take_slot(
        &self,
        tree: &TreeId,
        outputs: &[String],
        evict: bool,
        total: u32,
    ) -> Result<Option<BuildSlot>> {
        loop {
            let candidate = {
                let mut locked = self.lock();
                let mut rows = store::all(&locked.db).map_err(|error| store_error(&error))?;
                let row = rows
                    .iter()
                    .find(|row| row.id == tree.as_str())
                    .ok_or_else(|| Error::NotFound(format!("build tree {tree}")))?;
                if ![
                    store::State::Leased,
                    store::State::Landing,
                    store::State::Prebuilding,
                ]
                .iter()
                .any(|state| row.state == state.as_str())
                {
                    return Err(Error::Conflict(format!(
                        "build tree {tree} is {}",
                        row.state
                    )));
                }
                locked
                    .build_space
                    .outputs
                    .entry(tree.clone())
                    .or_default()
                    .extend(outputs.iter().cloned());
                let held = self.occupied(&locked, row);
                rows.retain(|row| self.occupied(&locked, row));
                if held || (!locked.build_space.reclaiming && rows.len() < total as usize) {
                    *locked.build_space.active.entry(tree.clone()).or_default() += 1;
                    return Ok(Some(BuildSlot {
                        trees: self.clone(),
                        tree: tree.clone(),
                    }));
                }
                if !evict || locked.build_space.reclaiming {
                    return Ok(None);
                }
                rows.sort_by(|a, b| a.last_used.cmp(&b.last_used).then_with(|| a.id.cmp(&b.id)));
                let mut candidate = None;
                for row in rows.iter().filter(|row| Self::reclaimable(&locked, row)) {
                    if let Some(guard) = try_exclusive(Path::new(&row.path))?
                        && self.tree_processes_clear(&disk::id_of(row)?, Path::new(&row.path))?
                    {
                        candidate = Some((row.clone(), guard));
                        break;
                    }
                }
                let Some((row, guard)) = candidate else {
                    return Ok(None);
                };
                let id = disk::id_of(&row)?;
                if !store::transition(
                    &locked.db,
                    &row.id,
                    store::State::Free,
                    store::State::Resetting,
                    &self.now(),
                )
                .map_err(|error| store_error(&error))?
                {
                    return Ok(None);
                }
                locked.build_space.reclaiming = true;
                locked.build_space.measuring.remove(&id);
                (row.clone(), guard, id, self.directories(&locked, &row))
            };
            let (mut row, mut guard, id, directories) = candidate;
            self.inner.builds.forget(&id);
            let cleared = invalidate_artifacts(&mut guard).and_then(|()| {
                directories
                    .iter()
                    .try_for_each(|dir| disk::remove_output(dir))
            });
            row.built_commit = None;
            drop(guard);
            let restored = self.put_back(&row);
            self.lock().build_space.reclaiming = false;
            restored?;
            cleared?;
            self.report_cleanup(format!("cleared {id}'s build directories for a build slot"));
        }
    }

    /// The measurement runs after the command, and only a later build in this tree waits for it.
    pub(crate) fn measure_build(&self, tree: &TreeId, output: Option<PathBuf>) {
        let mut locked = self.lock();
        let Ok(Some(row)) = store::get(&locked.db, tree.as_str()) else {
            return;
        };
        let dirs = self.directories(&locked, &row);
        let (send, receive) = mpsc::channel();
        if thread::Builder::new()
            .name(format!("measure-{tree}"))
            .spawn(move || {
                let _ = send.send(disk::Measurement::read(&dirs, output.as_deref()));
            })
            .is_ok()
        {
            locked.build_space.measuring.insert(tree.clone(), receive);
        } else {
            locked.build_space.measuring.remove(tree);
        }
    }

    /// Clean only when the recorded build exceeded the ceiling. On restart (or after a seed)
    /// get a baseline from this tree before writing more output to it.
    pub(crate) fn prepare_build(
        &self,
        tree: &TreeId,
        ceiling: u64,
        stopped: &dyn Fn() -> bool,
    ) -> Result<()> {
        let (row, dirs, measured) = {
            let mut locked = self.lock();
            let row = store::get(&locked.db, tree.as_str())
                .map_err(|error| store_error(&error))?
                .ok_or_else(|| Error::NotFound(format!("build tree {tree}")))?;
            let dirs = self.directories(&locked, &row);
            (row, dirs, locked.build_space.measuring.remove(tree))
        };
        let measured = if let Some(receiver) = measured {
            loop {
                match receiver.recv_timeout(std::time::Duration::from_millis(250)) {
                    Ok(measurement) if measurement.bytes.is_ok() => break measurement,
                    Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break disk::Measurement::read(&dirs, None);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) if stopped() => {
                        self.lock()
                            .build_space
                            .measuring
                            .entry(tree.clone())
                            .or_insert(receiver);
                        return Err(Error::Conflict(format!(
                            "stopped waiting for the size of {tree}'s build outputs"
                        )));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
            }
        } else {
            disk::Measurement::read(&dirs, None)
        };
        if measured.bytes? <= ceiling {
            return Ok(());
        }
        let path = Path::new(&row.path);
        let waiting = crate::steps::waiting(format!(
            "commands to finish in {tree} before cleaning its build outputs"
        ));
        let mut guard = loop {
            if stopped() {
                return Err(Error::Conflict(format!(
                    "stopped waiting to clean the build outputs in {tree}"
                )));
            }
            if !Seeding::involves(path)
                && !Self::legacy_command_in(&self.lock(), path)
                && let Some(guard) = try_exclusive(path)?
                && self.tree_processes_clear(tree, path)?
            {
                break guard;
            }
            thread::sleep(std::time::Duration::from_millis(250));
        };
        drop(waiting);
        let _step = crate::steps::during(format!("cleaning {tree} under its build ceiling"));
        invalidate_artifacts(&mut guard)?;
        // Clear cached successes before deleting anything, in case this process stops partway.
        self.inner.builds.forget(tree);
        {
            let locked = self.lock();
            if let Some(mut current) =
                store::get(&locked.db, &row.id).map_err(|error| store_error(&error))?
            {
                current.built_commit = None;
                store::upsert(&locked.db, &current).map_err(|error| store_error(&error))?;
            }
        }
        let dirs = self.directories(&self.lock(), &row);
        disk::clean(&dirs, ceiling, &measured.fingerprints, |stage| {
            self.report_cleanup(format!(
                "cleaned {stage} in {tree} to enforce the build ceiling"
            ));
        })?;
        Ok(())
    }

    fn report_cleanup(&self, text: String) {
        eprintln!("ignite: {text}");
        self.lock().build_space.last_cleanup = Some((self.inner.clock.now(), text));
    }
}
