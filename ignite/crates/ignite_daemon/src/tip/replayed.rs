//! Reverting a landing that was replayed over changes its gate never saw (`[revert] replayed`,
//! see "A broken build is reverted" in `docs/ignite.md`).
//!
//! A landing by verdict (`land::by_verdict`) runs its gate on the base its ticket tree had,
//! then replays the change onto the `staging` tip in git's object database and pushes it
//! without running anything again. When the landings in between changed what a check reads,
//! the pushed commit is a combination of code that no gate ran.
//!
//! On 2026-11-28 a landing of phone layout fixes passed every `client-interface` check on base
//! `d9a0a6e` and was pushed over one landing (`7e97768`, new mana effects and their timing)
//! that had landed while its gate ran. The next tip failed seven phone checks, and the culprit
//! search found that each passed at `7e97768` and failed at the landing. Those checks had no
//! recorded product catch, so the stall rule could not revert, and `master` did not move for
//! more than four hours while four regression tickets fixed the checks one at a time. In the
//! seven days before, 15 regressions had the same shape: the culprit's own gate had passed the
//! failing check, and the culprit had been pushed replayed over 1 to 14 landings.
//!
//! Such a failure says nothing against the check: both sides of the combination passed it. It
//! says the landing on `staging` is not the code its gate passed. The round reverts it at once,
//! with the module `revert`'s other conditions (one culprit, the check fails again at the tip
//! without its cache and passes at the revert, the revert builds). That takes `staging` back to
//! commits that each passed a gate, and the reopened ticket lands again with its tree rebased
//! onto the tip for real and the gate run there, so the conflict is found before `staging`.

use std::collections::BTreeSet;

use ignite_core::ids::{CheckName, Commit};

use super::{Culprit, TipCheck, TipLanding, TipVerifier, touches};
use crate::server::Context;

/// A landing between `master` and the tip that was pushed replayed onto a newer tip than its
/// gate's base, and the landings it was replayed over.
pub(super) struct Replay {
    pub(super) landing: TipLanding,
    pub(super) over: Vec<TipLanding>,
}

impl Replay {
    /// Whether a landing this one was replayed over changed what `check` reads.
    pub(super) fn touches(&self, check: &TipCheck) -> bool {
        self.over
            .iter()
            .any(|over| touches(&over.paths, &check.inputs))
    }

    /// Why the landing is reverted for `check`, for its reopened ticket, or `None` when no
    /// landing it was replayed over changed what `check` reads.
    pub(super) fn reason(&self, check: &TipCheck) -> Option<String> {
        let base = self.landing.gated_base.as_ref()?;
        let named: Vec<String> = self
            .over
            .iter()
            .filter(|over| touches(&over.paths, &check.inputs))
            .map(|over| {
                let first = over.commits.first().map_or("", |c| short(c));
                match &over.ticket {
                    Some(ticket) => format!("{ticket} ({first})"),
                    None => first.to_owned(),
                }
            })
            .collect();
        (!named.is_empty()).then(|| {
            format!(
                "Reverted because this landing's gate passed on {} and the landing was then \
                 pushed onto a newer tip without the gate running again. Landings it was \
                 replayed over changed what {} reads: {}. The check passes just before this \
                 landing and fails with it, so the combination is what fails. Rebase onto the \
                 tip and land again: the gate now runs on the rebased tree.",
                short(base),
                check.check,
                named.join(", ")
            )
        })
    }
}

impl TipVerifier {
    /// Which of `candidates`, the failing checks a revert could undo, a replayed landing
    /// between `master` and `tip` may have broken: some landing it was replayed over changed
    /// what the check reads. Empty without `[revert] replayed`.
    pub(super) fn replay_revertible<'a>(
        &self,
        ctx: &Context,
        master: &Commit,
        tip: &Commit,
        candidates: impl Iterator<Item = &'a TipCheck>,
    ) -> BTreeSet<CheckName> {
        let candidates: Vec<&TipCheck> = candidates.collect();
        if !ctx.config.revert.replayed || candidates.is_empty() || master == tip {
            return BTreeSet::new();
        }
        let replays = self.replays(ctx, master, tip);
        candidates
            .into_iter()
            .filter(|check| replays.iter().any(|replay| replay.touches(check)))
            .map(|check| check.check.clone())
            .collect()
    }

    /// Why the culprit `found` of `check` is reverted as a replayed landing (see the
    /// module), or `None` when it is not one: the search did not end at one landing, the
    /// landing was not replayed, or no landing it was replayed over changed what `check` reads.
    pub(super) fn replay_reason(
        &self,
        ctx: &Context,
        check: &TipCheck,
        found: &Culprit,
    ) -> Option<String> {
        let [landing] = found.suspects.as_slice() else {
            return None;
        };
        self.replay_of(ctx, landing.clone())?.reason(check)
    }

    /// The landings between `master` and `tip` that were pushed replayed over others, each
    /// with the landings it was replayed over. Empty when they cannot be read.
    pub(super) fn replays(&self, ctx: &Context, master: &Commit, tip: &Commit) -> Vec<Replay> {
        let landings = match self.work.landings(ctx, master, tip) {
            Ok(landings) => landings,
            Err(error) => {
                eprintln!("ignite: tip {tip}: landings for replay reverts: {error}");
                return Vec::new();
            }
        };
        landings
            .into_iter()
            .filter_map(|landing| self.replay_of(ctx, landing))
            .collect()
    }

    /// `landing` with the landings it was replayed over, or `None` when it was not replayed or
    /// they cannot be read.
    pub(super) fn replay_of(&self, ctx: &Context, landing: TipLanding) -> Option<Replay> {
        let base = landing.gated_base.as_ref()?;
        let last = landing.commits.last()?;
        if base == last {
            return None;
        }
        let over: Vec<TipLanding> = match self.work.landings(ctx, base, last) {
            Ok(found) => found
                .into_iter()
                .filter(|other| {
                    !other
                        .commits
                        .iter()
                        .any(|commit| landing.commits.contains(commit))
                })
                .collect(),
            Err(error) => {
                eprintln!("ignite: landings replayed over by {last}: {error}");
                return None;
            }
        };
        (!over.is_empty()).then_some(Replay { landing, over })
    }
}

fn short(commit: &Commit) -> &str {
    let text = commit.as_str();
    &text[..text.len().min(7)]
}
