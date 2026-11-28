//! In a round that failed, running the checks past the tip budget that its culprit may have
//! broken, before the round reports (see "Tip verification" in `docs/ignite.md`).
//!
//! A round runs only the checks within `[tip] seconds` before `master` moves, and the rest
//! after it moves (see `split`). A round that fails never runs the rest, and the next tip scores
//! its checks again and picks a different slice. On 2026-11-28 one phone layout landing broke 30
//! checks of one family, and twenty rounds in a row each found one to eight of them, so `master`
//! stood still for over eight hours while upkeep switched them off one batch at a time.
//!
//! Now, when the culprit search pins a reported failure in family F on platform P on one landing
//! L, the round also runs every check of F on P past the budget whose inputs L changed, and
//! reports their failures in the same round against the same culprit, so that upkeep switches
//! them off together. The sweep is bounded only by the family's size: a whole family costs less
//! than the extra round each batch would otherwise cost. It runs in the `tip` class without the
//! tip reserve, whatever newer tip appears, and ends when `master` reaches the tip.

use std::collections::{HashMap, HashSet};

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;

use super::coverage::Scope;
use super::culprit::{BlameEntry, Blamed};
use super::families::RunPhase;
use super::meter::Tally;
use super::{TipCheck, TipRun, TipVerifier, settle_runs, split, touches};
use crate::server::Context;

/// The checks of `after` that a culprit in `named` may have broken and that have no verdict yet:
/// those of the culprit's family and platform whose inputs its landing changed.
fn chosen<'a>(
    after: &[&'a TipCheck],
    named: &[Blamed],
    verdicts: &HashMap<ResultKey, TipRun>,
) -> Vec<&'a TipCheck> {
    after
        .iter()
        .copied()
        .filter(|check| {
            check.runnable
                && verdicts
                    .get(&check.key)
                    .is_none_or(|run| run.outcome == Outcome::Incomplete)
                && named.iter().any(|blamed| {
                    blamed.family == check.family
                        && blamed.platform == check.key.platform
                        && touches(&blamed.landing.paths, &check.inputs)
                })
        })
        .collect()
}

impl TipVerifier {
    /// Runs the checks past the budget that the culprits in `named` may have broken, and
    /// reports their failures (see the module documentation). An error is printed: the round
    /// has its verdict already, and the next round runs what this one could not.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sweep(
        &mut self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        scope: &Scope<'_>,
        checks: &[TipCheck],
        after: &[&TipCheck],
        named: &[Blamed],
        regressions: &HashSet<CheckName>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
        tally: &mut Tally,
    ) {
        let chosen = chosen(after, named, verdicts);
        if chosen.is_empty() {
            return;
        }
        eprintln!(
            "ignite: tip {tip} failed; running the {} checks past the budget its culprit may \
             have broken",
            chosen.len()
        );
        let (ended, runs, elsewhere) = match self.run_families(ctx, tip, &chosen, RunPhase::Sweep) {
            Ok(ran) => ran,
            Err(error) => {
                eprintln!("ignite: tip {tip}: the sweep past the budget stopped: {error}");
                return;
            }
        };
        if let Some(budget) = tally.budget.as_mut() {
            budget.swept = u32::try_from(runs.len()).unwrap_or(u32::MAX);
            budget.unrun = split::unrun(after.iter(), &runs, &elsewhere);
        }
        split::note_elsewhere(tally, &elsewhere);
        tally.ran = tally
            .ran
            .saturating_add(u32::try_from(runs.len()).unwrap_or(u32::MAX));
        settle_runs(checks, runs, verdicts, &mut self.own_outputs);
        if let Some(at) = ended {
            eprintln!("ignite: tip {tip}: the sweep past the budget ended, master is at {at}");
            return;
        }
        let failed: Vec<&TipCheck> = chosen
            .into_iter()
            .filter(|check| {
                verdicts
                    .get(&check.key)
                    .is_some_and(|run| run.outcome == Outcome::Fail)
            })
            .collect();
        let at_master = self.passed_at_master(ctx, scope, last, tip, &failed);
        let entries: Vec<BlameEntry<'_>> = failed
            .iter()
            .map(|check| BlameEntry {
                check,
                prior: scope
                    .last_good_for(check)
                    .filter(|_| !at_master.contains(&check.check)),
                output: verdicts
                    .get(&check.key)
                    .and_then(|run| run.output.as_deref()),
                allow_revert: false,
                replayed_only: false,
            })
            .collect();
        if let Err(error) = self.blame_failed(ctx, last, tip, regressions, &entries, true) {
            eprintln!("ignite: tip {tip}: failures past the budget not all reported: {error}");
        }
    }
}
