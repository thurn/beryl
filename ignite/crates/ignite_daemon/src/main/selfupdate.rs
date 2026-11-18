//! Keeps the running daemon on the current release of Ignite.
//!
//! Three things make the daemon restart into another executable:
//!
//! - The release branch (`master`, unless the repository names a `release` branch) moves and
//!   files under Ignite's own source directory changed. The tip loop calls
//!   [`SelfUpdate::after_master_moved`], which starts building the new executable in the update
//!   tree; the build is stored under its commit when it has finished. The daemon restarts into it within one round of the sender loop
//!   (10 seconds), whatever runs. A daemon
//!   whose own commit the branch does not contain, such as one installed by hand from
//!   `staging`, stays where it is: going to the branch's tip would be a downgrade.
//! - Someone pushes straight to the release branch, which tip verification never sees. The
//!   sender loop calls [`SelfUpdate::poll_release`] every round, and every
//!   [`POLL_EVERY_ROUNDS`]th call fetches the branch's tip and prepares the build the same way.
//! - The hub reports, in its heartbeat reply, a version other than the daemon's. The sender loop
//!   calls [`SelfUpdate::after_heartbeat`], which builds the hub's version (or finds it stored)
//!   and restarts into it.
//!
//! The hub updates first, but a daemon can see `master` move before the hub has restarted. A
//! daemon that already runs `master`'s tip therefore waits for the hub instead of going back to
//! the hub's older version.
//!
//! The release is built by a thread of its own, through the daemon's launcher (see
//! [`crate::update::ReleaseBuilder`]), so the sender loop goes on sending heartbeats and the outbox
//! while Cargo runs, which takes minutes on a loaded machine. Whichever loop asks for a build
//! starts it unless one runs already. The sender looks each round whether it has finished
//! ([`SelfUpdate::collect_build`]) and says in its step that a release is building. A build that
//! failed is reported once and not tried again for the same commit; the next move of the release
//! branch tries the new commit.
//!
//! A restart does not wait for commands. Supervisors are other processes, started from the old
//! executable's own path, so a restart leaves them running until their commands finish. The new
//! daemon runs the verbs that were cut off again under their handles, taking their commands over
//! from the supervisors (see `server/resume.rs` and [`crate::adopt`]), and a client whose
//! connection dropped waits on its handle again. The only thing a restart waits for is a verb
//! in a part that may not be cut short, such as a landing's push (see
//! [`crate::steps::critical`]), which lasts seconds.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Git, Result};

use crate::update::{
    Builder, ExecutableStore, Plan, Restart, SOURCE, UpdateCheck, Updater, restart_into,
};

/// The sender loop runs every 10 seconds, so this many of its rounds are 5 minutes.
pub const POLL_EVERY_ROUNDS: u32 = 30;

/// A release build running on its own thread.
pub struct Building {
    pub commit: Commit,
    handle: JoinHandle<Result<PathBuf>>,
}

/// The daemon's view of its own version and the release it should run.
pub struct SelfUpdate {
    /// The commit this daemon was built from, or `None` for an executable that is not in the
    /// store (one run straight from a Cargo target directory), which never updates itself.
    pub own: Option<Commit>,
    pub git: Arc<dyn Git>,
    pub builder: Arc<dyn Builder + Send + Sync>,
    pub store: ExecutableStore,
    /// The branch releases come from: the repository's release branch, `master` by default.
    pub master: String,
    pub restart: Box<dyn Restart + Send + Sync>,
    /// A stored build of a newer `master`, which the next heartbeat restarts into.
    pub ready: Mutex<Option<PathBuf>>,
    /// How many times [`SelfUpdate::poll_release`] has been called.
    pub rounds: AtomicU32,
    /// The release tip last reported as not containing this daemon's commit.
    pub ahead_of: Mutex<Option<Commit>>,
    /// The release build that is running, if any. At most one runs, so a second request for the
    /// same commit (the tip loop and the sender can both ask) starts nothing.
    pub building: Mutex<Option<Building>>,
    /// The commit whose build last failed, which is not built again until another commit is
    /// wanted.
    pub failed: Mutex<Option<Commit>>,
}

impl SelfUpdate {
    fn updater(&self) -> Updater<'_> {
        Updater {
            git: self.git.as_ref(),
            builder: self.builder.as_ref(),
            store: &self.store,
            master: &self.master,
            source: SOURCE,
        }
    }

    fn ready(&self) -> std::sync::MutexGuard<'_, Option<PathBuf>> {
        self.ready.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// After tip verification moved `master`: builds and stores the new executable if Ignite's
    /// source changed. The restart comes with the next [`SelfUpdate::after_heartbeat`].
    pub fn after_master_moved(&self) -> Result<()> {
        let Some(own) = &self.own else {
            return Ok(());
        };
        if self.ready().is_some() {
            // A newer build already waits; once the daemon runs it, the next check goes on
            // from there.
            return Ok(());
        }
        match self.updater().plan(own)? {
            Plan::Build(tip) => {
                if let Some(executable) = self.stage(own, &tip)? {
                    *self.ready() = Some(executable);
                }
            }
            Plan::Done(UpdateCheck::Ahead { tip }) => {
                // Said once per tip, not on every poll.
                let mut seen = self.ahead_of.lock().unwrap_or_else(PoisonError::into_inner);
                if seen.as_ref() != Some(&tip) {
                    eprintln!(
                        "ignite: staying on {own}: the release branch {} is at {tip}, which \
                         does not contain it",
                        self.master
                    );
                    *seen = Some(tip);
                }
            }
            Plan::Done(_) => {}
        }
        Ok(())
    }

    /// The stored executable of `target`, activated, or `None` when it is not stored: a build is
    /// then started on a thread of its own, unless one runs already or this commit's build
    /// failed, and [`SelfUpdate::collect_build`] picks up its result.
    fn stage(&self, own: &Commit, target: &Commit) -> Result<Option<PathBuf>> {
        if self.store.is_rejected(target) {
            return Ok(None);
        }
        if self.store.has(target) {
            return match self.updater().activate(own, target)? {
                UpdateCheck::Ready { executable, .. } => Ok(Some(executable)),
                _ => Ok(None),
            };
        }
        self.start_build(target);
        Ok(None)
    }

    fn building(&self) -> MutexGuard<'_, Option<Building>> {
        self.building.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn failed(&self) -> MutexGuard<'_, Option<Commit>> {
        self.failed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts building `target` unless a build runs (of this commit or another, which finishes
    /// first) or the last build of `target` failed.
    fn start_build(&self, target: &Commit) {
        let mut building = self.building();
        if building.is_some() || self.failed().as_ref() == Some(target) {
            return;
        }
        let (builder, commit) = (self.builder.clone(), target.clone());
        let spawned = thread::Builder::new()
            .name("ignite-release-build".to_owned())
            .spawn(move || builder.build(&commit));
        match spawned {
            Ok(handle) => {
                eprintln!("ignite: building release {target}");
                *building = Some(Building {
                    commit: target.clone(),
                    handle,
                });
            }
            Err(error) => {
                eprintln!("ignite: cannot start the build of release {target}: {error}");
                *self.failed() = Some(target.clone());
            }
        }
    }

    /// Looks, without waiting, whether the release build has finished. While it runs, records
    /// "building release <commit>" as the calling loop's step. A finished build is stored and
    /// becomes the ready build; a failed one is reported once and its commit is not built again.
    pub fn collect_build(&self) {
        let Some(own) = &self.own else {
            return;
        };
        let mut slot = self.building();
        let Some(building) = slot.take() else {
            return;
        };
        if !building.handle.is_finished() {
            crate::steps::step(format!("building release {}", building.commit));
            *slot = Some(building);
            return;
        }
        drop(slot);
        let Building { commit, handle } = building;
        let built = handle
            .join()
            .unwrap_or_else(|_| Err(Error::Io("the build thread panicked".to_owned())))
            .and_then(|built| self.updater().install(own, &commit, &built));
        match built {
            Ok(UpdateCheck::Ready { executable, .. }) => {
                eprintln!("ignite: built release {commit}");
                *self.ready() = Some(executable);
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("ignite: the release {commit} was not built: {error}");
                *self.failed() = Some(commit);
            }
        }
    }

    /// Called once per round of the sender loop. Every [`POLL_EVERY_ROUNDS`]th call (the first
    /// one included) fetches the release branch's tip and, if it moved past this daemon's own
    /// commit and files under Ignite's source changed, builds and stores it exactly as
    /// [`SelfUpdate::after_master_moved`] does. This is what deploys a push made straight to
    /// the release branch. A commit recorded as bad is never built.
    pub fn poll_release(&self) -> Result<()> {
        if !self
            .rounds
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(POLL_EVERY_ROUNDS)
        {
            return Ok(());
        }
        self.after_master_moved()
    }

    /// After each heartbeat, with the hub's version from the last reply that arrived. Restarts
    /// the daemon into a ready build of `master`, or into the hub's version when it differs,
    /// without waiting for commands. `critical` says that a verb is in a part that may not be cut
    /// short (a push); the restart then waits for the next heartbeat.
    pub fn after_heartbeat(&self, hub_version: Option<&str>, critical: bool) {
        let Some(own) = &self.own else {
            return;
        };
        self.collect_build();
        // A hub that reports no commit, such as one run from an unstored executable, asks for
        // no update.
        // A release that was rolled back for failing to start is not updated to again.
        let hub = hub_version
            .and_then(|v| Commit::new(v).ok())
            .filter(|hub| !self.store.is_rejected(hub))
            .filter(|hub| hub != own);
        let executable = match hub {
            None => match self.ready().clone() {
                None => return,
                Some(executable) => executable,
            },
            Some(hub) => match self.hub_executable(own, &hub) {
                Ok(Some(executable)) => executable,
                Ok(None) => return,
                Err(error) => {
                    eprintln!("ignite: cannot update to the hub's version {hub}: {error}");
                    return;
                }
            },
        };
        if critical {
            eprintln!(
                "ignite: {} is ready; restarting once a push in progress ends",
                executable.display()
            );
            return;
        }
        eprintln!("ignite: restarting into {}", executable.display());
        let error = restart_into(self.restart.as_ref(), &executable);
        // Only a failed restart returns. Forget the build so the next heartbeat does not
        // retry it every few seconds; the next move of `master` prepares it again.
        eprintln!("ignite: {error}");
        *self.ready() = None;
    }

    /// The stored executable of the hub's version, built now if it is not stored, or `None`
    /// while this daemon already runs `master`'s tip and the hub has yet to catch up.
    fn hub_executable(&self, own: &Commit, hub: &Commit) -> Result<Option<PathBuf>> {
        if self.git.tip(&self.master)? == *own {
            return Ok(None);
        }
        self.stage(own, hub)
    }
}

#[cfg(test)]
#[path = "selfupdate_tests.rs"]
mod tests;
