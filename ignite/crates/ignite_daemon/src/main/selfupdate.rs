//! Keeps the running daemon on the current release of Ignite.
//!
//! Three things make the daemon restart into another executable:
//!
//! - The release branch (`master`, unless the repository names a `release` branch) moves and
//!   files under Ignite's own source directory changed. The tip loop calls
//!   [`SelfUpdate::after_master_moved`], which builds the new executable in the update tree and
//!   stores it under its commit. The daemon restarts into it once no command runs.
//! - Someone pushes straight to the release branch, which tip verification never sees. The
//!   sender loop calls [`SelfUpdate::poll_release`] every round, and every
//!   [`POLL_EVERY_ROUNDS`]th call fetches the branch's tip and prepares the build the same way.
//! - The hub reports, in its heartbeat reply, a version other than the daemon's. The sender loop
//!   calls [`SelfUpdate::after_heartbeat`]: while commands run, the daemon asks for no new work;
//!   once none runs, it builds the hub's version (or finds it stored) and restarts into it.
//!
//! The hub updates first, but a daemon can see `master` move before the hub has restarted. A
//! daemon that already runs `master`'s tip therefore waits for the hub instead of going back to
//! the hub's older version.
//!
//! Supervisors are other processes, started from the old executable's own path, so a restart
//! leaves them running until their commands finish.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use ignite_core::ids::Commit;
use ignite_core::traits::{Git, Result};

use crate::update::{
    Builder, ExecutableStore, Restart, UpdateCheck, Updater, VersionAction, restart_into,
    version_action,
};

/// The directory of Ignite's own source in the repository; a change under it means a new build.
pub const SOURCE: &str = "ignite";

/// The sender loop runs every 10 seconds, so this many of its rounds are 5 minutes.
pub const POLL_EVERY_ROUNDS: u32 = 30;

/// The daemon's view of its own version and the release it should run.
pub struct SelfUpdate {
    /// The commit this daemon was built from, or `None` for an executable that is not in the
    /// store (one run straight from a Cargo target directory), which never updates itself.
    pub own: Option<Commit>,
    pub git: Arc<dyn Git>,
    pub builder: Box<dyn Builder + Send + Sync>,
    pub store: ExecutableStore,
    /// The branch releases come from: the repository's release branch, `master` by default.
    pub master: String,
    pub restart: Box<dyn Restart + Send + Sync>,
    /// A stored build of a newer `master`, waiting for running commands to finish.
    pub ready: Mutex<Option<PathBuf>>,
    /// How many times [`SelfUpdate::poll_release`] has been called.
    pub rounds: AtomicU32,
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
    /// source changed. The restart waits for [`SelfUpdate::after_heartbeat`].
    pub fn after_master_moved(&self) -> Result<()> {
        let Some(own) = &self.own else {
            return Ok(());
        };
        if self.ready().is_some() {
            // A newer build already waits; once the daemon runs it, the next check goes on
            // from there.
            return Ok(());
        }
        if let UpdateCheck::Ready { executable, .. } = self.updater().check(own)? {
            *self.ready() = Some(executable);
        }
        Ok(())
    }

    /// Called once per round of the sender loop. Every [`POLL_EVERY_ROUNDS`]th call (the first
    /// one included) fetches the release branch's tip and, if it moved past this daemon's own
    /// commit and files under Ignite's source changed, builds and stores it exactly as
    /// [`SelfUpdate::after_master_moved`] does. This is what deploys a push made straight to
    /// the release branch. A commit recorded as bad is never built.
    pub fn poll_release(&self) -> Result<()> {
        if self.rounds.fetch_add(1, Ordering::Relaxed) % POLL_EVERY_ROUNDS != 0 {
            return Ok(());
        }
        self.after_master_moved()
    }

    /// After each heartbeat, with the hub's version from the last reply that arrived and the
    /// number of commands running. Restarts the daemon when it should and nothing runs. Returns
    /// whether the daemon may ask the hub for work.
    pub fn after_heartbeat(&self, hub_version: Option<&str>, running: usize) -> bool {
        let Some(own) = &self.own else {
            return true;
        };
        // A hub that reports no commit, such as one run from an unstored executable, asks for
        // no update.
        // A release that was rolled back for failing to start is not updated to again.
        let hub = hub_version
            .and_then(|v| Commit::new(v).ok())
            .filter(|hub| !self.store.is_rejected(hub));
        let action = hub.as_ref().map_or(VersionAction::Current, |hub| {
            version_action(own, hub.as_str(), running)
        });
        let executable = match action {
            VersionAction::Drain => return false,
            VersionAction::Current => match self.ready().clone() {
                None => return true,
                Some(_) if running > 0 => return false,
                Some(executable) => executable,
            },
            VersionAction::UpdateNow => {
                let hub = hub.expect("only a hub version that differs asks for an update");
                match self.hub_executable(own, &hub) {
                    Ok(Some(executable)) => executable,
                    Ok(None) => return false,
                    Err(error) => {
                        eprintln!("ignite: cannot update to the hub's version {hub}: {error}");
                        return false;
                    }
                }
            }
        };
        eprintln!("ignite: restarting into {}", executable.display());
        let error = restart_into(self.restart.as_ref(), &executable);
        // Only a failed restart returns. Forget the build so the next heartbeat does not
        // retry it every few seconds; the next move of `master` prepares it again.
        eprintln!("ignite: {error}");
        *self.ready() = None;
        false
    }

    /// The stored executable of the hub's version, built now if it is not stored, or `None`
    /// while this daemon already runs `master`'s tip and the hub has yet to catch up.
    fn hub_executable(&self, own: &Commit, hub: &Commit) -> Result<Option<PathBuf>> {
        if self.git.tip(&self.master)? == *own {
            return Ok(None);
        }
        match self.updater().prepare(own, hub)? {
            UpdateCheck::Ready { executable, .. } => Ok(Some(executable)),
            UpdateCheck::UpToDate | UpdateCheck::NotNeeded { .. } => Ok(None),
        }
    }
}

#[cfg(test)]
#[path = "selfupdate_tests.rs"]
mod tests;
