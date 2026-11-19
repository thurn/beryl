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
//! On the hub's machine the hub is a login service of its own (`ignite hub`), started from
//! `bin/current`. Before the daemon there restarts into a release, it kickstarts the hub's
//! service, so the hub comes back on the new build within a second and reports it to every
//! other daemon. If the hub still reports another version than a daemon that runs
//! `bin/current` (its restart failed, or the hub service was installed after the release), the
//! daemon kickstarts it again, at most every [`HUB_KICK_GAP_S`] seconds, and does not follow it
//! back to the older version.
//!
//! The release is built by a thread of its own, through the daemon's launcher (see
//! [`crate::update::ReleaseBuilder`]), so the sender loop goes on sending heartbeats and the outbox
//! while Cargo runs, which takes minutes on a loaded machine. Whichever loop asks for a build
//! starts it unless one runs already. The sender looks each round whether it has finished
//! ([`SelfUpdate::collect_build`]) and says in its step that a release is building. A build that
//! failed is reported once and not tried again for the same commit; the next move of the release
//! branch tries the new commit.
//!
//! The same thread then runs the new executable's selftest (`ignite internal selftest`, see
//! [`crate::update::selftest`]) before `bin/current` is pointed at it. A build that fails it is
//! recorded as rejected, so no daemon restarts into it or updates to it again, and is reported
//! to the justiciar once (the report is kept and sent again if the hub does not answer). The
//! daemon stays on its own build.
//!
//! A restart does not wait for commands or for the long verbs. Supervisors are other processes,
//! started from the old executable's own path, so a restart leaves them running until their
//! commands finish, and the long verbs (`land`, `test`, `check`, `run`, `rebuild`, `audit`) run
//! in verb processes of their own, which go on with the new daemon (see `server/process.rs`).
//! Before it restarts, the daemon stops taking new requests, which clients send again, and waits
//! up to [`crate::server::DRAIN_WAIT`] for the verbs running on its own threads, such as a
//! `release` ([`crate::server::Drain`]). A `start` that only waits for a free tree is not waited
//! for: the new daemon runs it again. A verb still running on the daemon's threads after the
//! wait is run again by the new daemon under its handle, taking its commands over from the
//! supervisors (see `server/resume.rs` and [`crate::adopt`]), and a client whose connection
//! dropped waits on its handle again. A restart also waits for a verb on the daemon's threads in
//! a part that may not be cut short (see [`crate::steps::critical`]), which lasts seconds.
//!
//! What the self-update is doing is shown by `ignite status` and waited for by
//! `ignite wait deploy-<commit>` (see [`crate::deploy`]), through its [`DeployState`]: the tip its
//! last check of the release branch saw and what that found, when it checks next, the build and
//! selftest running, a build ready to restart into, and why the last build failed. A deploy wait
//! asks for the branch to be checked at the next round ([`DeployState::check_soon`]), so a
//! landing that moved the release is picked up within seconds instead of at the regular check.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};

use ignite_core::ids::{Commit, MachineId};
use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Error, Git, Hub, Result};

use crate::deploy::{Checked, DeployState, LocalRelease, Work};
use crate::server::{DRAIN_WAIT, Drain};
use crate::update::selftest::Selftest;
use crate::update::{
    Builder, ExecutableStore, Plan, Restart, SOURCE, UpdateCheck, Updater, restart_into,
};

/// The sender loop runs every 10 seconds, so this many of its rounds are 5 minutes.
pub const POLL_EVERY_ROUNDS: u32 = 30;

/// Seconds between two rounds of the sender loop, which calls [`SelfUpdate::poll_release`].
const ROUND_S: f64 = 10.0;

/// A hub that lags behind the daemon on its machine is kickstarted at most this often.
pub const HUB_KICK_GAP_S: f64 = 300.0;

/// The hub's login service on this machine, which runs `bin/current hub`.
pub trait HubService: Send + Sync {
    /// Whether this machine has the hub's login service. Asked each time, so that a service
    /// installed while the daemon runs is used.
    fn installed(&self) -> bool;
    /// Kills the hub and starts it again from `bin/current`.
    fn kickstart(&self) -> std::io::Result<()>;
}

/// A release build running on its own thread.
pub struct Building {
    pub commit: Commit,
    handle: JoinHandle<std::result::Result<PathBuf, Failure>>,
    /// Set by the thread once the executable is stored and its selftest runs.
    testing: Arc<AtomicBool>,
}

/// Why a release was not made ready.
enum Failure {
    /// Cargo, or storing the executable, failed. The commit is not tried again, but a rebuild
    /// after the branch moves may work.
    Build(Error),
    /// The executable was built and failed its selftest, with the reason.
    Selftest(String),
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
    /// The commit whose build last failed, and why. It is not built again until another commit
    /// is wanted.
    pub failed: Mutex<Option<(Commit, String)>>,
    /// Checks a built executable before the daemon restarts into it.
    pub selftest: Arc<dyn Selftest>,
    /// Where selftest failures are reported.
    pub hub: Arc<dyn Hub>,
    pub clock: Arc<dyn Clock>,
    pub machine: MachineId,
    pub host: String,
    /// Selftest failures the hub has not accepted yet.
    pub reports: Mutex<Vec<JusticiarReport>>,
    /// The hub's login service, on the hub's machine; restarted before the daemon.
    pub hub_service: Arc<dyn HubService>,
    /// When the hub was last kickstarted for lagging behind this daemon.
    pub hub_kicked: Mutex<Option<Timestamp>>,
    /// The server, set once it exists, which stops taking requests and lets the verbs on the
    /// daemon's threads finish before a restart.
    pub drain: OnceLock<Arc<dyn Drain>>,
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
                if let Some(executable) = self.stage(&tip) {
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

    /// The executable of `target` if it has passed its selftest and been activated, else `None`:
    /// a thread of its own then builds it (when it is not stored) and runs its selftest, unless
    /// one runs already or this commit failed, and [`SelfUpdate::collect_build`] picks up the
    /// result.
    fn stage(&self, target: &Commit) -> Option<PathBuf> {
        if self.store.is_rejected(target) {
            return None;
        }
        // A build that passed its selftest and was activated is not checked again.
        let stored = self.store.path(target);
        if self.ready().as_ref() == Some(&stored) {
            return Some(stored);
        }
        self.start_build(target);
        None
    }

    fn building(&self) -> MutexGuard<'_, Option<Building>> {
        self.building.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn failed(&self) -> MutexGuard<'_, Option<(Commit, String)>> {
        self.failed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts making `target` ready on a thread of its own, unless that is running (for this
    /// commit or another, which finishes first) or the last attempt for `target` failed: builds
    /// it when it is not stored, then runs its selftest.
    fn start_build(&self, target: &Commit) {
        let mut building = self.building();
        if building.is_some() || self.failed().as_ref().map(|(commit, _)| commit) == Some(target) {
            return;
        }
        let (builder, store, selftest, commit) = (
            self.builder.clone(),
            self.store.clone(),
            self.selftest.clone(),
            target.clone(),
        );
        let testing = Arc::new(AtomicBool::new(false));
        let tested = testing.clone();
        let spawned = thread::Builder::new()
            .name("ignite-release-build".to_owned())
            .spawn(move || {
                if !store.has(&commit) {
                    let built = builder.build(&commit).map_err(Failure::Build)?;
                    store
                        .store(&commit, &built)
                        .map_err(|e| Failure::Build(e.into()))?;
                }
                let executable = store.path(&commit);
                tested.store(true, Ordering::SeqCst);
                selftest
                    .run(&executable)
                    .map_err(|e| Failure::Selftest(e.to_string()))?;
                Ok(executable)
            });
        match spawned {
            Ok(handle) => {
                eprintln!("ignite: building release {target}");
                *building = Some(Building {
                    commit: target.clone(),
                    handle,
                    testing,
                });
            }
            Err(error) => {
                eprintln!("ignite: cannot start the build of release {target}: {error}");
                self.record_failure(target, &format!("cannot start a thread: {error}"));
            }
        }
    }

    /// Looks, without waiting, whether the release build has finished. While it runs, records
    /// "building release <commit>" as the calling loop's step. A finished build that passed its
    /// selftest is activated and becomes the ready build. A failed build is reported once and
    /// its commit is not built again. A build that failed its selftest is also recorded as
    /// rejected and reported to the justiciar. Also sends justiciar reports the hub has not
    /// accepted yet.
    pub fn collect_build(&self) {
        let Some(own) = &self.own else {
            return;
        };
        self.send_reports();
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
        let Building { commit, handle, .. } = building;
        let built = handle.join().unwrap_or_else(|_| {
            Err(Failure::Build(Error::Io(
                "the build thread panicked".to_owned(),
            )))
        });
        match built {
            Ok(_) => match self.updater().activate(own, &commit) {
                Ok(UpdateCheck::Ready { executable, .. }) => {
                    eprintln!("ignite: built release {commit}");
                    *self.ready() = Some(executable);
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("ignite: the release {commit} was not activated: {error}");
                    self.record_failure(&commit, &error.to_string());
                }
            },
            Err(Failure::Build(error)) => {
                eprintln!("ignite: the release {commit} was not built: {error}");
                self.record_failure(&commit, &error.to_string());
            }
            Err(Failure::Selftest(reason)) => {
                eprintln!("ignite: the release {commit} failed its selftest: {reason}");
                self.store
                    .reject_with_reason(&commit, &reason, self.clock.now());
                self.reports
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(self.selftest_report(&commit, &reason));
                self.record_failure(&commit, &reason);
                self.send_reports();
            }
        }
    }

    /// Records that making `commit` ready failed, for `reason`: it is not built again until
    /// another commit is wanted.
    fn record_failure(&self, commit: &Commit, reason: &str) {
        *self.failed() = Some((commit.clone(), reason.to_owned()));
    }

    fn selftest_report(&self, commit: &Commit, reason: &str) -> JusticiarReport {
        JusticiarReport {
            subject: format!("release-selftest:{commit}"),
            title: format!("Ignite release {commit} failed its selftest"),
            details: format!(
                "The daemon on {} built Ignite {commit}, ran `ignite internal selftest` with it \
                 and it failed, so no daemon restarts into it: it is recorded in `bin/rejected` \
                 and `bin/rollbacks.log` there, and the daemon stays on {}. The selftest parses \
                 `ignite.toml` and the check manifests and applies the new build's migrations to \
                 a copy of `daemon.db`. Fix the cause with a regression test and push a new \
                 release through the release branch.\n\n{reason}",
                self.host,
                self.own.as_ref().map_or("its build", Commit::as_str),
            ),
            evidence: format!("{commit}@{}", self.host),
            machine: self.machine.clone(),
            host: self.host.clone(),
            on_machine: false,
        }
    }

    /// Sends the reports the hub has not accepted. A report the hub refuses or cannot be
    /// reached for stays for the next round.
    fn send_reports(&self) {
        let pending =
            std::mem::take(&mut *self.reports.lock().unwrap_or_else(PoisonError::into_inner));
        let mut unsent = Vec::new();
        for report in pending {
            if let Err(error) = self.hub.justiciar(&report) {
                eprintln!("ignite: cannot report {}: {error}", report.subject);
                unsent.push(report);
            }
        }
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(unsent);
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
        let reported = hub_version.and_then(|v| Commit::new(v).ok());
        // On the hub's machine a hub on another build than the daemon is the one to move, even
        // onto a rejected build: after a rollback the hub must leave the bad release too, or
        // every other daemon would follow it there.
        if let Some(hub) = reported.as_ref().filter(|hub| *hub != own)
            && self.hub_lags_behind(own)
        {
            self.catch_the_hub_up(hub);
            return;
        }
        let hub = reported
            .clone()
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
        self.restart_hub_first(reported.as_ref(), &executable);
        self.drain_and_restart(&executable);
        // Only a failed restart returns. Forget the build so the next heartbeat does not
        // retry it every few seconds; the next move of `master` prepares it again.
        *self.ready() = None;
    }

    /// Restarts the daemon into `executable` now, by the same steps as a restart into a release,
    /// for `ignite internal restart`, which drills a restart into the daemon's own executable:
    /// waits while `critical` says a verb is in a part that may not be cut short, stops taking
    /// requests, waits for the verbs on the daemon's threads, records a clean stop and restarts.
    /// Returns only if the restart failed, after taking requests again.
    pub fn restart_now(&self, executable: &std::path::Path, critical: &dyn Fn() -> bool) {
        while critical() {
            thread::sleep(std::time::Duration::from_secs(1));
        }
        self.drain_and_restart(executable);
    }

    /// Stops taking requests, waits up to [`DRAIN_WAIT`] for the verbs on the daemon's threads,
    /// records a clean stop and restarts into `executable`. Returns only if the restart failed,
    /// after taking requests again.
    fn drain_and_restart(&self, executable: &std::path::Path) {
        let drain = self.drain.get();
        if let Some(drain) = drain {
            drain.drain(DRAIN_WAIT);
        }
        eprintln!("ignite: restarting into {}", executable.display());
        crate::update::record_clean_stop_from(&self.store);
        let error = restart_into(self.restart.as_ref(), executable);
        eprintln!("ignite: {error}");
        if let Some(drain) = drain {
            drain.undrain();
        }
    }

    /// Whether this is the hub's machine and the daemon runs `bin/current`, which the hub's
    /// service starts too: a hub on another version is then the one behind.
    fn hub_lags_behind(&self, own: &Commit) -> bool {
        self.hub_service.installed() && self.store.current_commit().as_ref() == Some(own)
    }

    /// Kickstarts the hub that reports `hub`, behind this daemon, unless that was done less than
    /// [`HUB_KICK_GAP_S`] ago.
    fn catch_the_hub_up(&self, hub: &Commit) {
        let now = self.clock.now();
        let mut kicked = self
            .hub_kicked
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if kicked.is_some_and(|at| (now.millis() - at.millis()) as f64 / 1000.0 < HUB_KICK_GAP_S) {
            return;
        }
        *kicked = Some(now);
        eprintln!("ignite: the hub runs {hub}, behind this daemon; restarting the hub");
        if let Err(error) = self.hub_service.kickstart() {
            eprintln!("ignite: cannot restart the hub: {error}");
        }
    }

    /// On the hub's machine, kickstarts the hub's service before the daemon restarts into
    /// `executable`, unless the hub already `reported` that version. `bin/current` already
    /// points at it, so the hub comes back on the new build and tells every other daemon. A hub
    /// that fails to restart does not hold the daemon back; the restarted daemon tries again
    /// (see [`SelfUpdate::catch_the_hub_up`]).
    fn restart_hub_first(&self, reported: Option<&Commit>, executable: &std::path::Path) {
        let target = ExecutableStore::version_of(executable);
        if target.is_none() || reported == target.as_ref() || !self.hub_service.installed() {
            return;
        }
        eprintln!("ignite: restarting the hub before the daemon");
        if let Err(error) = self.hub_service.kickstart() {
            eprintln!("ignite: cannot restart the hub: {error}");
        }
    }

    /// The stored executable of the hub's version, built now if it is not stored, or `None`
    /// while this daemon already runs `master`'s tip and the hub has yet to catch up.
    fn hub_executable(&self, own: &Commit, hub: &Commit) -> Result<Option<PathBuf>> {
        if self.git.tip(&self.master)? == *own {
            return Ok(None);
        }
        Ok(self.stage(hub))
    }
}

impl DeployState for SelfUpdate {
    fn local(&self) -> LocalRelease {
        let building = self
            .building()
            .as_ref()
            .map(|b| (b.commit.clone(), b.testing.load(Ordering::SeqCst)));
        let work = match building {
            Some((commit, true)) => Some(Work::SelfTesting(commit)),
            Some((commit, false)) => Some(Work::Building(commit)),
            None => match self
                .ready()
                .as_deref()
                .and_then(ExecutableStore::version_of)
            {
                Some(commit) => Some(Work::Restarting(commit)),
                None => self
                    .failed()
                    .clone()
                    .map(|(commit, reason)| Work::Failed(commit, reason)),
            },
        };
        // The call of `poll_release` that finds a multiple of `POLL_EVERY_ROUNDS` checks the
        // branch, one round of the sender loop after another.
        let rounds = self.rounds.load(Ordering::Relaxed);
        let left = (POLL_EVERY_ROUNDS - rounds % POLL_EVERY_ROUNDS) % POLL_EVERY_ROUNDS;
        LocalRelease {
            own: self.own.clone(),
            branch: self.master.clone(),
            next_check: Some(self.clock.now().plus_seconds(f64::from(left) * ROUND_S)),
            work,
        }
    }

    fn check(&self) -> Result<(Commit, Checked)> {
        let tip = self.git.tip(&self.master)?;
        let Some(own) = &self.own else {
            return Ok((tip, Checked::Current));
        };
        Ok(match self.updater().plan(own)? {
            Plan::Build(tip) => (tip, Checked::Build),
            Plan::Done(UpdateCheck::Ahead { tip }) => (tip, Checked::Ahead),
            Plan::Done(UpdateCheck::NotNeeded { tip }) => (tip, Checked::Unchanged),
            Plan::Done(UpdateCheck::Rejected { tip }) => (tip, Checked::Rejected),
            Plan::Done(UpdateCheck::UpToDate | UpdateCheck::Ready { .. }) => {
                (tip, Checked::Current)
            }
        })
    }

    /// The next call of [`SelfUpdate::poll_release`], within a round of the sender loop, checks
    /// the branch.
    fn check_soon(&self) {
        self.rounds.store(0, Ordering::Relaxed);
    }

    /// A version that holds the target's changes to Ignite's source as other commits, as the
    /// daemon still running a hand-pushed commit does once the release is its rebased copy,
    /// runs the release too.
    fn reached(&self, version: &Commit, target: &Commit) -> bool {
        version == target
            || self.git.is_ancestor(target, version).unwrap_or(false)
            || self
                .git
                .holds_changes(version, target, SOURCE)
                .unwrap_or(false)
    }

    fn rejection(&self, commit: &Commit) -> Option<String> {
        if !self.store.is_rejected(commit) {
            return None;
        }
        Some(
            self.store
                .rejection(commit)
                .unwrap_or_else(|| format!("{commit} is listed in bin/rejected")),
        )
    }
}

#[cfg(test)]
#[path = "selfupdate_tests.rs"]
mod tests;
