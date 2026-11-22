//! Sweeping leased trees that run nothing.
//!
//! On 2026-11-20 jarvis's disk filled with every build waiting for space. The trees' build
//! directories held about 1,800 GB under a 250 GB limit, single trees 150 to 200 GB, and the disk
//! loop could free none of it: it swept only free trees, and under full load nearly every tree
//! was leased. A tree stays leased for as long as its ticket's session works, which is hours, and
//! its builds grow all that time.
//!
//! The disk loop also sweeps a leased tree whenever nothing runs in it: no verb was asked
//! from it, no process has its working directory or executable inside it, and the build manager
//! agreed to start no build there until the sweep ends ([`Builds::pause`]). It deletes replaced
//! builds and stale object files there as in a free tree. Below the emergency free-space floor,
//! it can also delete incremental caches and then whole build directories. Doing that every
//! time the working set exceeded its budget forced the same lease to rebuild on its next command.
//!
//! [`Builds::pause`]: ignite_core::traits::Builds::pause

use std::path::PathBuf;

use ignite_core::ids::TreeId;
use ignite_core::traits::Result;
use ignite_os::Native;
use ignite_os::occupants::Occupants as _;
use ignite_store::local::{rows, trees as store};

use super::super::{Trees, store_error};
use super::{id_of, profile_dirs, remove};

/// The folder of rustc's incremental caches in each profile directory of a Cargo target
/// directory. Cargo rebuilds what is missing from it at the next compile, so it can be deleted
/// whenever no compile runs.
const INCREMENTAL: &str = "incremental";

/// Keeps the lease in place while its idleness is checked and its build files are swept.
pub(super) struct Paused<'a> {
    trees: &'a Trees,
    id: TreeId,
    pub path: PathBuf,
    resume_builds: bool,
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        if self.resume_builds {
            self.trees.inner.builds.resume(&self.id);
        }
        self.trees
            .finish_ticket_tree_work(&mut self.trees.lock(), &self.id);
    }
}

impl Trees {
    /// The leased trees, leased longest ago first.
    pub(super) fn leased_rows(&self) -> Result<Vec<rows::Tree>> {
        let mut leased: Vec<rows::Tree> = store::all(&self.lock().db)
            .map_err(|e| store_error(&e))?
            .into_iter()
            .filter(|row| row.state == store::State::Leased.as_str())
            .collect();
        leased.sort_by(|a, b| a.leased_at.cmp(&b.leased_at).then(a.id.cmp(&b.id)));
        Ok(leased)
    }

    /// Pauses builds in the leased tree `id` if nothing runs in it ([`Builds::pause`]), and
    /// returns the guard that resumes them. `None` when a verb runs from the tree, a build runs
    /// in it, a process works inside it, its processes cannot be listed, or a seed copies into
    /// it or from it (the daemon's own copy is not a process inside the tree).
    ///
    /// [`Builds::pause`]: ignite_core::traits::Builds::pause
    pub(super) fn pause_idle(&self, id: &TreeId) -> Option<Paused<'_>> {
        self.pause_or_why(id).ok()
    }

    /// [`Trees::pause_idle`], or what keeps the tree busy, in words for the disk loop's report
    /// when it cannot get the disk back above its floor (`disk/floor.rs`).
    pub(super) fn pause_or_why(&self, id: &TreeId) -> std::result::Result<Paused<'_>, String> {
        let path = {
            let mut locked = self.lock();
            let row = store::get(&locked.db, id.as_str())
                .map_err(|error| store_error(&error).to_string())?
                .ok_or_else(|| "it is no longer recorded".to_owned())?;
            if row.state != store::State::Leased.as_str() {
                return Err(format!("it is {}", row.state));
            }
            // Reserve before taking any snapshots of running work, so all of the checks and
            // the deletion belong to the same lease.
            if !locked.ticket_tree_work.insert(id.clone()) {
                return Err("a checkout or disk sweep is already in progress".to_owned());
            }
            PathBuf::from(row.path)
        };
        let mut paused = Paused {
            trees: self,
            id: id.clone(),
            path,
            resume_builds: false,
        };
        if super::super::Seeding::involves(&paused.path) {
            return Err("a seed copies builds into it or out of it".to_owned());
        }
        let verbs = self.inner.steps.in_tree(id, &paused.path);
        if !verbs.is_empty() {
            return Err(verbs
                .iter()
                .map(|activity| describe_verb(activity))
                .collect::<Vec<_>>()
                .join("; "));
        }
        if !self.inner.builds.pause(id) {
            return Err("a build runs in it".to_owned());
        }
        paused.resume_builds = true;
        // Read after the pause, so a command that starts building between the two is seen.
        match Native.occupants(&paused.path) {
            Ok(found) if !found.is_empty() => {
                let pids: Vec<String> = found.iter().map(|o| o.pid.to_string()).collect();
                return Err(format!(
                    "processes work inside it (pid {})",
                    pids.join(", ")
                ));
            }
            Ok(_) => {}
            Err(error) => return Err(format!("its processes cannot be listed: {error}")),
        }
        Ok(paused)
    }

    /// Calls `sweep` with each idle leased tree's build directories, paused while it runs.
    pub(super) fn each_idle_leased(&self, mut sweep: impl FnMut(&PathBuf)) -> Result<()> {
        for row in self.leased_rows()? {
            let Some(paused) = self.pause_idle(&id_of(&row)?) else {
                continue;
            };
            for dir in self.inner.config.keep.get().iter() {
                sweep(&paused.path.join(dir));
            }
        }
        Ok(())
    }

    /// Deletes the incremental caches of every idle leased tree that has any, and returns the
    /// trees it deleted them from.
    pub(super) fn drop_incremental(&self) -> Result<Vec<TreeId>> {
        let mut dropped = Vec::new();
        for row in self.leased_rows()? {
            let id = id_of(&row)?;
            let Some(paused) = self.pause_idle(&id) else {
                continue;
            };
            let mut any = false;
            for dir in self.inner.config.keep.get().iter() {
                for profile in profile_dirs(&paused.path.join(dir)) {
                    let caches = profile.join(INCREMENTAL);
                    if caches.is_dir() {
                        remove(&caches);
                        any = true;
                    }
                }
            }
            if any {
                dropped.push(id);
            }
        }
        Ok(dropped)
    }

    /// Deletes the build directories of the leased tree `id` if nothing runs in it. Returns
    /// whether it did.
    pub(super) fn clear_idle_leased(&self, id: &TreeId) -> Result<bool> {
        let Some(paused) = self.pause_idle(id) else {
            return Ok(false);
        };
        // Forgotten first, as for a free tree (see `hold_disk`). The release records the tree
        // as built at its head only while its row says it holds a build, and a tree with no
        // build must not be chosen as the one built nearest the tip.
        self.inner.builds.forget(id);
        {
            let locked = self.lock();
            let row = store::get(&locked.db, id.as_str()).map_err(|e| store_error(&e))?;
            if let Some(mut row) = row.filter(|row| row.built_commit.is_some()) {
                row.built_commit = None;
                store::upsert(&locked.db, &row).map_err(|e| store_error(&e))?;
            }
        }
        for dir in self.inner.config.keep.get().iter() {
            remove(&paused.path.join(dir));
        }
        Ok(true)
    }
}

/// A running verb as the floor report names it: what it is, for which ticket, and whether it
/// only waits for disk space, which is the case where it waits on the disk loop itself.
fn describe_verb(activity: &crate::steps::Activity) -> String {
    let name = match activity.kind() {
        crate::steps::Kind::Verb(verb) => verb.clone(),
        crate::steps::Kind::Loop(name) => name.clone(),
    };
    let ticket = activity
        .work()
        .ticket
        .map(|ticket| format!(" for {ticket}"))
        .unwrap_or_default();
    let waits_for_disk = activity
        .waits()
        .iter()
        .any(|wait| wait.starts_with(crate::build::DISK_WAIT));
    if waits_for_disk {
        format!("`{name}`{ticket} waits for disk space to start a build")
    } else {
        format!("`{name}`{ticket} runs from it")
    }
}
