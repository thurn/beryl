//! Moving the release branch after a landing (see `release` in `ignite.toml`).
//!
//! When a landing's commits change Ignite's own source, the landed commit may also move the
//! release branch, which daemons build their executable from. It moves only if every check of
//! the release family passed on that commit. The landing decides that before it pushes
//! ([`Landing::judge_release`]):
//!
//! - An express landing (see [`super::express`]) has already run the express commands on the
//!   commit, which is what this step asks of the release family, so the release moves on their
//!   pass.
//! - A gate that ran every check of the family counts. Only the family's own checks matter: a
//!   gate that left checks of other families to tip verification still counts for this one.
//! - Otherwise the landing runs the whole family in the gate's tree before it takes the lease,
//!   building any product the family needs that the tree has never built. A gate does not list
//!   a family whose listing needs such a product, so without this step the family ran after the
//!   push, with the ticket tree still leased and the agent waiting for its reply: two minutes
//!   for Ignite's own family under load. A family run that passes is added to the gate's
//!   outcomes. One that fails does not refuse the landing; the release just stays where it is.
//!
//! After the push, the ticket's tree is released and the hub told before the release branch is
//! pushed ([`Landing::move_release`]), so the only work left before the reply is that one push.
//! It is not handed to a thread: `ignite land` runs in a verb process that exits once it has
//! replied (see `crate::verbproc`), and a failed move must reach the `release` line. Whatever
//! happens here is reported on that line and never fails or undoes the landing.
//!
//! The push fast-forwards the release branch. When the release branch holds a commit the landed
//! commit does not contain, the push is refused; if the landed commit holds that commit's
//! changes to Ignite's source anyway, the branch is moved over it with `--force-with-lease`.
//! That is the state a commit pushed to the release branch by hand leaves: its landing rebases
//! it, and the release branch holds the copy from before the rebase. Before this, the release
//! stayed on that copy until someone moved it by hand.
//!
//! A landing that moved the release names the deploy handle in its `next` line
//! ([`crate::deploy::landed_handle`]): `ignite wait deploy-<commit>` waits until every machine
//! runs the release or it was rolled back (see [`crate::deploy`]).

use ignite_core::ids::Commit;
use ignite_core::protocol::Outcome;
use ignite_core::traits::Push;

use super::env::GateTree;
use super::{Landing, line, short};
use crate::checks::{CheckRun, GateRun, RunOptions};
use crate::server::Context;
use crate::update::SOURCE;

/// Whether the landed commit may move the release branch, decided before the push: `Ok` when
/// every check of the release family passed, or `Err` with the reason it may not.
pub(super) type Verdict = Result<(), String>;

impl Landing {
    /// The release branch and family a landing that changed `files` must judge, or `None`
    /// when the repository names no release or the files are all outside Ignite's source.
    fn release_of<'a>(ctx: &'a Context, files: &[String]) -> Option<(&'a str, &'a str)> {
        let repository = &ctx.config.repository;
        let (Some(branch), Some(family)) = (&repository.release, &repository.release_family) else {
            return None;
        };
        let source = format!("{SOURCE}/");
        files
            .iter()
            .any(|file| file.starts_with(&source))
            .then_some((branch.as_str(), family.as_str()))
    }

    /// Decides, before the push, whether the commit in `tree` may move the release branch;
    /// `None` when the landing does not call for a move. A gate that did not run every check of
    /// the release family has the family run now in `tree`, and a passing run is added to
    /// `gated`, so the landing's output counts it with the gate.
    pub(super) fn judge_release(
        &self,
        ctx: &Context,
        tree: &GateTree,
        files: &[String],
        gated: &mut GateRun,
        options: &RunOptions,
        express: bool,
    ) -> Option<Verdict> {
        let (_, family) = Self::release_of(ctx, files)?;
        if express {
            return Some(Ok(()));
        }
        let outcomes = if Self::gate_ran_family(gated, family) {
            Self::family_outcomes(gated, family)
        } else {
            crate::steps::step(format!("running the {family} checks in {}", tree.tree));
            let run = self.gate.run_family(ctx, tree, family, options);
            if !run.passed() {
                return Some(Err(format!("{family} checks did not pass")));
            }
            let outcomes = run.outcomes.iter().map(|(_, outcome)| *outcome).collect();
            Self::add_family_run(gated, family, run);
            outcomes
        };
        Some(if outcomes.is_empty() {
            Err(format!("{family} lists no checks"))
        } else if outcomes
            .iter()
            .all(|outcome| matches!(outcome, Outcome::Pass | Outcome::Flaky))
        {
            Ok(())
        } else {
            Err(format!("{family} checks did not pass"))
        })
    }

    /// Adds a passing run of the whole release family to the gate's run: the family's checks
    /// are no longer left to tip verification.
    fn add_family_run(gated: &mut GateRun, family: &str, run: CheckRun) {
        let left = gated.left_by_family.remove(family).unwrap_or(0);
        gated.left = gated.left.saturating_sub(left);
        gated.run.cold_families.retain(|cold| cold != family);
        gated
            .run
            .outcomes
            .retain(|(check, _)| check.family() != family);
        gated.run.outcomes.extend(run.outcomes);
        gated.run.from_cache += run.from_cache;
        gated.run.ran += run.ran;
        gated.run.builds.extend(run.builds);
        gated.run.records.extend(run.records);
        gated.run.wall_s += run.wall_s;
    }

    /// Moves the release branch to the pushed commit if `verdict` allows it. Returns the
    /// `release` line for the landing's output, or `None` when the landing called for no move.
    pub(super) fn move_release(
        &self,
        ctx: &Context,
        tree: &GateTree,
        commit: &Commit,
        verdict: Option<Verdict>,
    ) -> Option<(String, String)> {
        let branch = ctx.config.repository.release.as_deref()?;
        if let Err(why) = verdict? {
            return Some(self.not_moved(branch, &why));
        }
        Some(match ctx.git.push(&tree.path, commit, branch) {
            Ok(Push::Pushed { .. }) => line("release", crate::deploy::moved(branch, commit)),
            Ok(Push::Rejected { reason }) => {
                self.replace_release(ctx, tree, commit, branch, &reason)
            }
            Err(error) => self.not_moved(branch, &error.to_string()),
        })
    }

    /// Moves the release branch to the landed commit over a commit it does not contain, when
    /// the landed commit holds that commit's changes to Ignite's source; the push was refused
    /// for `refused`.
    fn replace_release(
        &self,
        ctx: &Context,
        tree: &GateTree,
        commit: &Commit,
        branch: &str,
        refused: &str,
    ) -> (String, String) {
        let held = match ctx.git.tip(branch) {
            Ok(held) => held,
            Err(error) => return self.not_moved(branch, &format!("{refused}; {error}")),
        };
        match ctx.git.holds_changes(commit, &held, SOURCE) {
            Ok(true) => {}
            Ok(false) => {
                return self.not_moved(
                    branch,
                    &format!(
                        "it holds {}, whose changes to {SOURCE}/ the landing lacks",
                        short(&held)
                    ),
                );
            }
            Err(error) => return self.not_moved(branch, &format!("{refused}; {error}")),
        }
        match ctx.git.replace(&tree.path, commit, branch, &held) {
            Ok(Push::Pushed { .. }) => line("release", crate::deploy::moved(branch, commit)),
            Ok(Push::Rejected { reason }) => self.not_moved(branch, &reason),
            Err(error) => self.not_moved(branch, &error.to_string()),
        }
    }

    /// Whether the gate ran the whole release family: it listed the family, has outcomes for
    /// it, and left none of its checks to tip verification. Checks of other families that the
    /// gate left there do not matter.
    fn gate_ran_family(gated: &GateRun, family: &str) -> bool {
        !gated.run.cold_families.iter().any(|cold| cold == family)
            && gated.left_by_family.get(family).copied().unwrap_or(0) == 0
            && !Self::family_outcomes(gated, family).is_empty()
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
