//! Building a landing's release before it lands, and starting a round of the sender loop as soon
//! as there is release work for it.
//!
//! An express landing names the commit it is about to check in
//! [`crate::update::prebuild::CANDIDATE_REF`]. [`SelfUpdate::poll_candidate`] builds and stores
//! that commit's executable while the landing runs its express commands, without a selftest.
//! When the landing pushes and the release branch reaches the commit, [`SelfUpdate::start_build`]
//! finds it stored and only runs the selftest. On 2026-11-19 the release build took 36 s after
//! each Ignite landing, about as long as the landing's express commands.
//!
//! The sender loop also waited out its 10 seconds between rounds whatever happened, so a moved
//! release branch was seen up to 10 s late and a finished build was restarted into up to 10 s
//! late. [`SelfUpdate::nap`] ends the wait as soon as either happens.

use std::sync::PoisonError;
use std::time::{Duration, Instant};

use ignite_core::ids::Commit;

use super::{Failure, SelfUpdate};
use crate::update::SOURCE;
use crate::update::prebuild::CANDIDATE_REF;

/// How often [`SelfUpdate::nap`] looks for release work.
const LOOK_EVERY: Duration = Duration::from_secs(1);

impl SelfUpdate {
    /// Starts building the commit [`CANDIDATE_REF`] names, once per commit, when it is worth
    /// building ([`Self::worth_prebuilding`]) and no other build runs. A build already running
    /// is not interrupted: the candidate is then built when the release branch reaches it.
    pub(super) fn poll_candidate(&self) {
        let Ok(Some(candidate)) = self.git.local_ref(CANDIDATE_REF) else {
            return;
        };
        {
            let mut seen = self
                .candidate_seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if seen.as_ref() == Some(&candidate) {
                return;
            }
            *seen = Some(candidate.clone());
        }
        if self.worth_prebuilding(&candidate) {
            self.spawn_build(&candidate, true);
        }
    }

    /// Whether `candidate` would be a release this daemon builds: it is not stored or rejected
    /// already, it is not the release branch's tip (which builds as a release), it follows the
    /// daemon's version and it changes files under Ignite's source.
    fn worth_prebuilding(&self, candidate: &Commit) -> bool {
        let Some(own) = self.version() else {
            return false;
        };
        if self.store.has(candidate) || self.store.is_rejected(candidate) {
            return false;
        }
        if self.git.known_tip(&self.master).ok().as_ref() == Some(candidate) {
            return false;
        }
        if !self.git.is_ancestor(&own, candidate).unwrap_or(false) {
            return false;
        }
        let prefix = format!("{}/", SOURCE.trim_end_matches('/'));
        self.git
            .changed(&own, candidate)
            .is_ok_and(|changed| changed.iter().any(|path| path.starts_with(&prefix)))
    }

    /// After the candidate `commit` was built ahead of its landing: logs the result, and starts
    /// the build of the newest release that appeared meanwhile, which finds the candidate stored
    /// when the landing pushed it. A failed candidate is not recorded as a failed release: the
    /// release built from the same commit later reports its own failure.
    pub(super) fn collect_prebuild(
        &self,
        commit: &Commit,
        built: std::result::Result<std::path::PathBuf, Failure>,
    ) {
        match built {
            Ok(_) => eprintln!("ignite: built candidate {commit} ahead of its landing"),
            Err(Failure::Build(error)) => {
                eprintln!("ignite: the candidate {commit} was not built: {error}");
            }
            // A candidate runs no selftest.
            Err(Failure::Selftest(_)) => {}
        }
        let newest = self.window().newest().cloned();
        if let Some(newest) = newest {
            let _ = self.stage(&newest);
        }
    }

    /// Whether the sender loop has release work to do now: the release build finished, the
    /// release branch moved in this checkout, or a landing named a new candidate. Each is two
    /// reads of a ref at most, cheap enough to ask every second.
    pub fn round_due(&self) -> bool {
        if self.own.is_none() {
            return false;
        }
        let finished = self
            .building()
            .as_ref()
            .is_some_and(|building| building.handle.is_finished());
        if finished {
            return true;
        }
        let seen = |slot: &std::sync::Mutex<Option<Commit>>| {
            slot.lock().unwrap_or_else(PoisonError::into_inner).clone()
        };
        let tip = self.git.known_tip(&self.master).ok();
        if tip.is_some() && tip != seen(&self.release_seen) {
            return true;
        }
        let candidate = self.git.local_ref(CANDIDATE_REF).ok().flatten();
        candidate.is_some() && candidate != seen(&self.candidate_seen)
    }

    /// Waits up to `most` between two rounds of the sender loop, ending early when
    /// [`Self::round_due`] says there is release work.
    pub fn nap(&self, most: Duration) {
        let began = Instant::now();
        loop {
            let left = most.saturating_sub(began.elapsed());
            if left.is_zero() {
                return;
            }
            std::thread::sleep(left.min(LOOK_EVERY));
            if self.round_due() {
                return;
            }
        }
    }
}
