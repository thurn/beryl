//! Moving the release branch after a landing (see `release` in `ignite.toml`).
//!
//! When a landing has reached the integration branch and its commits changed Ignite's own
//! source, the commit may also move the release branch, which daemons build their executable
//! from. It moves only if every check of the release family passed on that commit: the landing
//! gate counts when it ran the whole family, and otherwise the family is run now in the tree
//! that holds the commit. Whatever happens here is reported on the landing's `release` line and
//! never fails or undoes the landing.
//!
//! A landing that moved the release names the deploy handle in its `next` line
//! ([`crate::deploy::landed_handle`]): `ignite wait deploy-<commit>` waits until every machine
//! runs the release or it was rolled back (see [`crate::deploy`]).

use ignite_core::ids::Commit;
use ignite_core::protocol::Outcome;
use ignite_core::traits::Push;

use super::env::GateTree;
use super::{Landing, line};
use crate::checks::{GateRun, RunOptions};
use crate::server::Context;
use crate::update::SOURCE;

/// What the release step needs to know about a landing that has pushed.
pub(super) struct Landed<'a> {
    /// The tree that holds the pushed commit.
    pub tree: &'a GateTree,
    pub commit: &'a Commit,
    /// The files the landed commits changed.
    pub files: &'a [String],
    pub gated: &'a GateRun,
    pub options: &'a RunOptions,
}

impl Landing {
    /// Moves the release branch if this landing calls for it. Returns the `release` line for the
    /// landing's output, or `None` when the landing needs nothing from the release branch.
    pub(super) fn move_release(
        &self,
        ctx: &Context,
        landed: &Landed<'_>,
    ) -> Option<(String, String)> {
        let repository = &ctx.config.repository;
        let (Some(branch), Some(family)) = (&repository.release, &repository.release_family) else {
            return None;
        };
        let source = format!("{SOURCE}/");
        if !landed.files.iter().any(|file| file.starts_with(&source)) {
            return None;
        }
        let outcomes: Vec<Outcome> = if Self::gate_ran_family(landed.gated, family) {
            Self::family_outcomes(landed.gated, family)
        } else {
            let run = self
                .gate
                .run_family(ctx, landed.tree, family, landed.options);
            if !run.passed() {
                return Some(self.not_moved(branch, &format!("{family} checks did not pass")));
            }
            run.outcomes.iter().map(|(_, outcome)| *outcome).collect()
        };
        if outcomes.is_empty() {
            return Some(self.not_moved(branch, &format!("{family} lists no checks")));
        }
        if !outcomes
            .iter()
            .all(|outcome| matches!(outcome, Outcome::Pass | Outcome::Flaky))
        {
            return Some(self.not_moved(branch, &format!("{family} checks did not pass")));
        }
        Some(
            match ctx.git.push(&landed.tree.path, landed.commit, branch) {
                Ok(Push::Pushed { .. }) => {
                    line("release", crate::deploy::moved(branch, landed.commit))
                }
                Ok(Push::Rejected { reason }) => self.not_moved(branch, &reason),
                Err(error) => self.not_moved(branch, &error.to_string()),
            },
        )
    }

    /// Whether the gate ran the whole release family: it has outcomes for the family and left
    /// no check to tip verification, so none of the family's checks was among those left.
    fn gate_ran_family(gated: &GateRun, family: &str) -> bool {
        gated.left == 0 && !Self::family_outcomes(gated, family).is_empty()
    }

    fn family_outcomes(gated: &GateRun, family: &str) -> Vec<Outcome> {
        gated
            .run
            .outcomes
            .iter()
            .filter(|(check, _)| check.family() == family)
            .map(|(_, outcome)| *outcome)
            .collect()
    }

    #[allow(clippy::unused_self)]
    fn not_moved(&self, branch: &str, why: &str) -> (String, String) {
        eprintln!("ignite: {branch} not moved: {why}");
        line("release", format!("{branch} not moved: {why}"))
    }
}
