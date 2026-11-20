//! Sweeping leased trees that run nothing.
//!
//! On 2026-11-20 jarvis's disk filled with every build waiting for space. The trees' build
//! directories held about 1,800 GB under a 250 GB limit, single trees 150 to 200 GB, and the disk
//! loop could free none of it: it swept only free trees, and under full load nearly every tree
//! was leased. A tree stays leased for as long as its ticket's session works, which is hours, and
//! its builds grow all that time.
//!
//! So the disk loop also sweeps a leased tree whenever nothing runs in it: no verb was asked
//! from it, no process has its working directory or executable inside it, and the build manager
//! agreed to start no build there until the sweep ends ([`Builds::pause`]). It deletes replaced
//! builds and stale object files there as in a free tree, and when the trees are still over
//! their limit with no free tree left to clear, the whole build directories of the idle leased
//! trees holding the most bytes of their own. The tree's next command then builds from scratch,
//! which is slower than an incremental build but keeps the disk from filling.
//!
//! [`Builds::pause`]: ignite_core::traits::Builds::pause

use std::path::PathBuf;

use ignite_core::ids::TreeId;
use ignite_core::traits::Result;
use ignite_os::Native;
use ignite_os::occupants::Occupants as _;
use ignite_store::local::{rows, trees as store};

use super::super::{Trees, store_error};
use super::{DiskReport, OWN_BYTES, Space, id_of, remove};

/// A leased tree whose builds the build manager holds back until this is dropped.
pub(super) struct Paused<'a> {
    trees: &'a Trees,
    pub id: TreeId,
    pub path: PathBuf,
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        self.trees.inner.builds.resume(&self.id);
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
        let path = self.inner.config.path(id);
        if super::super::Seeding::involves(&path, id)
            || !self.inner.steps.in_tree(id, &path).is_empty()
            || !self.inner.builds.pause(id)
        {
            return None;
        }
        let paused = Paused {
            trees: self,
            id: id.clone(),
            path,
        };
        // Read after the pause, so a command that starts building between the two is seen.
        let occupied = Native
            .occupants(&paused.path)
            .map_or(true, |found| !found.is_empty());
        let still_leased = store::get(&self.lock().db, id.as_str())
            .ok()
            .flatten()
            .is_some_and(|row| row.state == store::State::Leased.as_str());
        (!occupied && still_leased).then_some(paused)
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

    /// Deletes the whole build directories of idle leased trees, those holding the most bytes of
    /// their own first, until `report.after` is within `limit`. A tree whose deletion alone would
    /// free less than [`OWN_BYTES`], or a hundredth of the limit where that is less, is passed
    /// over: its files are clones of another tree's, and clearing it would cost its session a
    /// cold build for almost nothing.
    pub(super) fn reclaim_leased(
        &self,
        space: &mut Space,
        limit: u64,
        report: &mut DiskReport,
    ) -> Result<()> {
        let mut leased: Vec<(TreeId, u64)> = self
            .leased_rows()?
            .iter()
            .map(id_of)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|id| {
                let own = space.freed_with(&id, &std::collections::HashSet::new());
                (id, own)
            })
            .collect();
        leased.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let least = OWN_BYTES.min(limit / 100);
        for (id, own) in leased {
            if report.after <= limit {
                break;
            }
            if own < least || own == 0 {
                continue;
            }
            if self.clear_idle_leased(&id)? {
                report.after = report.after.saturating_sub(space.clear(&id));
                report.reclaimed.push(id);
            }
        }
        Ok(())
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
