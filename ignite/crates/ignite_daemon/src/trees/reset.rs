//! Ending a ticket tree's lease: stop its processes and reset it to a clean checkout.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, MutexGuard, PoisonError};
use std::time::Duration;

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_git::{refs, worktree};
use ignite_os::Native;
use ignite_os::occupants::Occupants;
use ignite_store::local::{handles as handle_store, rows, trees as store};

use super::{Ended, GRACE, Locked, Trees, park_branch, store_error};

impl Trees {
    /// Waits for checkout or sweeping to finish and returns with the store locked so the
    /// caller can end the lease before another sweep reserves the tree.
    pub(super) fn wait_for_ticket_tree_work<'a>(
        &self,
        locked: MutexGuard<'a, Locked>,
        tree: &TreeId,
    ) -> MutexGuard<'a, Locked> {
        if !locked.ticket_tree_work.contains(tree) {
            return locked;
        }
        let _waiting = crate::steps::waiting(format!("checkout or disk cleanup in {tree} to end"));
        self.inner
            .ticket_tree_work_done
            .wait_while(locked, |locked| locked.ticket_tree_work.contains(tree))
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn finish_ticket_tree_work(&self, locked: &mut Locked, tree: &TreeId) {
        locked.ticket_tree_work.remove(tree);
        self.inner.ticket_tree_work_done.notify_all();
    }

    pub(super) fn reset_leased(
        &self,
        tree: &TreeId,
        row: &rows::Tree,
        park_at: &Commit,
        caller: Option<u32>,
    ) -> Result<Vec<Ended>> {
        let config = &self.inner.config;
        let path = config.path(tree);
        let had_build = {
            let locked = self.wait_for_ticket_tree_work(self.lock(), tree);
            // Read again after waiting. The sweep may have cleared the build, or another
            // release may have reached the tree first.
            let current = store::get(&locked.db, tree.as_str()).map_err(|e| store_error(&e))?;
            let Some(current) = current.filter(|current| {
                current.leased_at == row.leased_at
                    && current.ticket == row.ticket
                    && current.session == row.session
            }) else {
                return Err(Error::Conflict(format!(
                    "tree {tree} is no longer held by this lease"
                )));
            };
            let claimed = store::transition(
                &locked.db,
                tree.as_str(),
                store::State::Leased,
                store::State::Resetting,
                &self.now(),
            )
            .map_err(|e| store_error(&e))?;
            if !claimed {
                return Err(Error::Conflict(format!(
                    "tree {tree} is no longer leased; another release has it"
                )));
            }
            current.built_commit.is_some()
        };
        // A release that fails after the tree is marked `resetting` goes back to `leased`, so
        // the caller can retry it; nothing else would move a `resetting` tree until the daemon
        // restarts.
        // A copy of another tree's build still running writes the target directories.
        let _ = super::seed::wait_until_seeded(&path);
        let reset = (|| {
            let head = refs::head(&path)?;
            self.stop_work(tree)?;
            let ended = Self::end_processes(&path, caller)?;
            worktree::reset(&path, &park_branch(tree), park_at, &config.keep.get())?;
            Ok((ended, head))
        })();
        let (ended, head) = match reset {
            Ok(reset) => reset,
            Err(error) => {
                self.back_to_leased(tree);
                return Err(error);
            }
        };
        let built = had_build.then(|| head.to_string());
        let freed = store::release(
            &self.lock().db,
            tree.as_str(),
            built.as_deref(),
            &self.now(),
        );
        if let Err(error) = freed {
            // The tree is clean but the store would not say so; leave it where a retry finds it.
            self.back_to_leased(tree);
            return Err(store_error(&error));
        }
        self.serve_waiters();
        Ok(ended)
    }

    /// Asks every verb still running from `tree` to stop, except the caller's own, and waits for
    /// each to end. A verb that refuses, or that has not ended within the wait, is an error, and
    /// the tree must not be freed: the next ticket would share it with that verb.
    fn stop_work(&self, tree: &TreeId) -> Result<()> {
        let own = crate::steps::current();
        let running: Vec<_> = self
            .inner
            .steps
            .in_tree(tree, &self.inner.config.path(tree))
            .into_iter()
            .filter(|activity| own.as_ref().is_none_or(|own| !Arc::ptr_eq(own, activity)))
            .collect();
        let name = |activity: &crate::steps::Activity| {
            let view = activity.view();
            match view.handle {
                Some(handle) => format!("`ignite {}` ({handle})", view.name),
                None => format!("`ignite {}`", view.name),
            }
        };
        let mut problems = Vec::new();
        for activity in &running {
            if let Err(reason) = activity.request_stop() {
                problems.push(format!("{} cannot be stopped: {reason}", name(activity)));
            }
        }
        for activity in &running {
            if !activity.stop_requested() {
                continue;
            }
            let since = std::time::Instant::now();
            if !activity.wait_finished(self.inner.stop_wait)
                || !self.process_exited(
                    activity,
                    self.inner.stop_wait.saturating_sub(since.elapsed()),
                )
            {
                problems.push(format!(
                    "{} did not end within {} s of being stopped",
                    name(activity),
                    self.inner.stop_wait.as_secs()
                ));
            }
        }
        if problems.is_empty() {
            return Ok(());
        }
        Err(Error::Conflict(format!(
            "{tree} is not freed: {}",
            problems.join("; ")
        )))
    }

    /// Waits up to `within` for the verb process of `activity`'s handle to exit, once its verb
    /// has written its reply, so nothing of it is left when the tree is reset. True at once for a
    /// verb that runs in the daemon, whose handle's row names no process.
    fn process_exited(&self, activity: &crate::steps::Activity, within: Duration) -> bool {
        let Some(handle) = activity.handle() else {
            return true;
        };
        let named = handle_store::get(&self.lock().db, handle.as_str())
            .ok()
            .flatten()
            .is_some_and(|row| row.pid.is_some());
        if !named {
            return true;
        }
        let home = crate::update::Home::from_env().root;
        let since = std::time::Instant::now();
        while crate::verbproc::alive(&home, &handle) {
            if since.elapsed() >= within {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    /// Puts a tree that a failed release left `resetting` back to `leased`.
    fn back_to_leased(&self, tree: &TreeId) {
        let _ = store::transition(
            &self.lock().db,
            tree.as_str(),
            store::State::Resetting,
            store::State::Leased,
            &self.now(),
        );
    }

    pub(super) fn row(&self, tree: &TreeId) -> Result<rows::Tree> {
        store::get(&self.lock().db, tree.as_str())
            .map_err(|e| store_error(&e))?
            .ok_or_else(|| Error::NotFound(format!("tree {tree}")))
    }

    /// Ends every process whose working directory or executable is inside `path`, except this
    /// process, `caller` and the ancestors of both, and every process in `caller`'s process
    /// group. The group is spared because the client usually runs in a pipeline such as
    /// `ignite close -m ... 2>&1 | tail -4` from a shell inside the tree: `tail` is neither the
    /// client nor its ancestor, and ending it ended the command line with exit 143.
    pub(super) fn end_processes(path: &Path, caller: Option<u32>) -> Result<Vec<Ended>> {
        let mut spared = BTreeSet::new();
        for start in [Some(std::process::id()), caller].into_iter().flatten() {
            let mut pid = Some(start);
            while let Some(current) = pid.filter(|&p| p > 1 && spared.insert(p)) {
                pid = Native.parent(current);
            }
        }
        // A caller in the daemon's own group would spare everything the daemon started.
        let caller_group = caller
            .and_then(|pid| Native.group(pid))
            .filter(|&group| group > 1 && Native.group(std::process::id()) != Some(group));
        let occupants: Vec<_> = Native
            .occupants(path)?
            .into_iter()
            .filter(|occupant| !spared.contains(&occupant.pid))
            .filter(|occupant| caller_group.is_none() || Native.group(occupant.pid) != caller_group)
            .collect();
        let pids: Vec<u32> = occupants.iter().map(|o| o.pid).collect();
        let killed = Native.end(&pids, GRACE)?;
        Ok(occupants
            .into_iter()
            .map(|occupant| Ended {
                killed: killed.contains(&occupant.pid),
                pid: occupant.pid,
                executable: occupant.executable,
                cwd: occupant.cwd,
            })
            .collect())
    }
}
