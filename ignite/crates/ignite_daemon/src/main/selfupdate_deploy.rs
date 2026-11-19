//! What the self-update shows in `ignite status` and waits for in `ignite wait
//! deploy-<commit>` (see [`crate::deploy`]): [`SelfUpdate`]'s [`DeployState`].

use std::sync::atomic::Ordering;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use super::{POLL_EVERY_ROUNDS, ROUND_S, SelfUpdate};
use crate::deploy::{Checked, DeployState, LocalRelease, Work};
use crate::update::{ExecutableStore, Plan, SOURCE, UpdateCheck};

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
                Some(commit) => {
                    let window = self.window();
                    Some(Work::Restarting {
                        commit,
                        due: window.due_at(),
                        cap: window.cap_at(),
                    })
                }
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
            own: self.version(),
            branch: self.master.clone(),
            next_check: Some(self.clock.now().plus_seconds(f64::from(left) * ROUND_S)),
            work,
        }
    }

    fn check(&self) -> Result<(Commit, Checked)> {
        let tip = self.git.tip(&self.master)?;
        let Some(own) = self.version() else {
            return Ok((tip, Checked::Current));
        };
        Ok(match self.updater().plan(&own)? {
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
