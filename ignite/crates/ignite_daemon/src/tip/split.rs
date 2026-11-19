//! The tip budget: which of a round's checks run before `master` moves, and running the rest
//! afterwards (see "Tip verification" in `docs/ignite.md`).
//!
//! - The checks a round still has to run are scored as the landing gate scores them, over the
//!   areas the landings since the last promoted tip touched, and chosen best first until
//!   `[tip] seconds` of running checks at the `tip` class's rate are spent. There is no
//!   threshold, and builds are outside the budget.
//! - Some checks run before `master` moves whatever the budget: a cached failure being run
//!   again, and a check with an open regression ticket. So a failure among the checks past the
//!   budget, once reported, holds `master` on every later round until its fix lands, as a
//!   cached failure does.
//! - When the checks within the budget, the forced ones and every check already answered pass,
//!   `master` moves. The checks past the budget then run in the same round, in the `tip` class
//!   on whatever cores are free. A failure among them is reported as a regression, which files
//!   its ticket; `master` has already moved and stays. A newer `staging` tip ends them at the
//!   next family or batch, even with batches under way, since the tip loop starts no round
//!   until this one ends.
//! - Checks only another platform's machines can run, and families that cannot be listed,
//!   still hold `master` as before: this machine cannot run them after the move, and the next
//!   round would find `master` at the tip and run nothing.
//! - When nothing can be scored (the hub or git cannot be read), every check runs before
//!   `master` moves, as before the budget.

use std::collections::{BTreeMap, HashMap, HashSet};

use ignite_core::budget::Budget;
use ignite_core::gate::{self, GateCheck, GateInput};
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::score::BILLIONS_PER_CPU_SECOND;
use ignite_core::tipround::RoundBudget;
use ignite_core::traits::Result;

use super::meter::Tally;
use super::{TipCheck, TipRun, TipVerifier, settle_runs};
use crate::checks::plan::class_rates;
use crate::server::Context;

/// A round's checks to run, split by the tip budget.
pub(super) struct Split<'a> {
    /// The checks to run before `master` moves: forced and within the budget.
    pub within: Vec<&'a TipCheck>,
    /// The checks to run after `master` moves.
    pub after: Vec<&'a TipCheck>,
    /// The split's figures; `None` when nothing could be scored.
    pub budget: Option<RoundBudget>,
    after_keys: HashSet<ResultKey>,
}

impl Split<'_> {
    /// Whether `check` waits until after `master` moves.
    pub fn is_after(&self, check: &TipCheck) -> bool {
        self.after_keys.contains(&check.key)
    }
}

impl TipVerifier {
    /// Splits `to_run` by the tip budget. `recheck`, the cached failures and checks the hub no
    /// longer trusts the cache for, run before `master` moves and are counted as forced.
    pub(super) fn split<'a>(
        &self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        to_run: &[&'a TipCheck],
        recheck: &[&TipCheck],
    ) -> Split<'a> {
        let unbudgeted = || Split {
            within: to_run.to_vec(),
            after: Vec::new(),
            budget: None,
            after_keys: HashSet::new(),
        };
        if to_run.is_empty() && recheck.is_empty() {
            return unbudgeted();
        }
        let asked: Vec<&TipCheck> = to_run.iter().chain(recheck).copied().collect();
        let Some(scoring) = self.work.scoring(ctx, last, tip, &asked) else {
            return unbudgeted();
        };
        let budget = Budget {
            threshold: 0.0,
            seconds: ctx.config.tip.seconds,
            rate: class_rates(ctx).tip(),
        };
        split_by(to_run, recheck, &scoring.areas, &scoring.checks, &budget)
    }

    /// Runs the checks past the budget once `master` has moved, and reports a failure among
    /// them as a regression. A newer `staging` tip ends this at the next family or batch, even
    /// with batches under way, so that the tip loop moves on to the round that can move
    /// `master` again; the checks not yet run are in the next tip's set, since nothing stored
    /// a result for them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_after(
        &mut self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        checks: &[TipCheck],
        split: &Split<'_>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
        tally: &mut Tally,
    ) -> Result<()> {
        tally.master_at = Some(ctx.clock.now());
        if split.after.is_empty() {
            return Ok(());
        }
        eprintln!(
            "ignite: master is at {tip}; running the {} checks past the tip budget",
            split.after.len()
        );
        let (superseded, runs) = self.run_families(ctx, tip, &split.after, true)?;
        // Until now every check past the budget counted as unrun (see `split_by`).
        let left = unrun(split.after.iter(), &runs);
        note_unrun(tally, left, superseded.clone());
        tally.ran = tally
            .ran
            .saturating_add(u32::try_from(runs.len()).unwrap_or(u32::MAX));
        settle_runs(checks, runs, verdicts, &mut self.own_outputs);
        if let Some(by) = superseded {
            eprintln!("ignite: checks past the tip budget at {tip} left to {by}");
            return Ok(());
        }
        Self::fill_from_cache(ctx, checks, verdicts)?;
        // Only these checks' verdicts: the others were noted before `master` moved.
        let after: HashMap<ResultKey, TipRun> = verdicts
            .iter()
            .filter(|(key, _)| split.after_keys.contains(*key))
            .map(|(key, run)| (key.clone(), run.clone()))
            .collect();
        self.note_incomplete(ctx, tip, checks, &after, false);
        for check in &split.after {
            let Some(run) = verdicts.get(&check.key) else {
                continue;
            };
            if run.outcome != Outcome::Fail {
                continue;
            }
            let output = run.output.clone();
            if let Err(error) = self.blame(ctx, last, tip, check, output.as_deref()) {
                eprintln!(
                    "ignite: regression of {} not reported: {error}",
                    check.check
                );
            }
        }
        Ok(())
    }
}

/// For each family, how many of `set` have no run among `runs`.
pub(super) fn unrun<'a>(
    set: impl Iterator<Item = &'a &'a TipCheck>,
    runs: &[TipRun],
) -> BTreeMap<String, u32> {
    let ran: HashSet<&CheckName> = runs.iter().map(|run| &run.check).collect();
    let mut left: BTreeMap<String, u32> = BTreeMap::new();
    for check in set.filter(|check| !ran.contains(&check.check)) {
        *left.entry(check.family.clone()).or_default() += 1;
    }
    left
}

/// Records in the round's budget the checks it set out to run and did not, and the newer tip
/// that ended it, if one did. The hub reads them to say why tip verification missed a check.
pub(super) fn note_unrun(tally: &mut Tally, left: BTreeMap<String, u32>, ended_by: Option<Commit>) {
    if let Some(budget) = tally.budget.as_mut() {
        budget.unrun = left;
        budget.ended_by = ended_by;
    }
}

/// The split of `to_run` by `budget`, with `scored` the hub's record of the checks over `areas`.
/// A check with no record (switched off since it was listed, say) runs before `master` moves.
fn split_by<'a>(
    to_run: &[&'a TipCheck],
    recheck: &[&TipCheck],
    areas: &[String],
    scored: &[GateCheck],
    budget: &Budget,
) -> Split<'a> {
    let record: HashMap<&CheckName, &GateCheck> = scored.iter().map(|c| (&c.check, c)).collect();
    let cost = |check: &GateCheck| {
        check
            .history
            .as_ref()
            .map_or(check.family_median_cost, |h| h.median_cost)
            .billions(BILLIONS_PER_CPU_SECOND)
    };
    let mut figures = RoundBudget {
        seconds: budget.seconds,
        rate: budget.rate,
        ..RoundBudget::default()
    };
    let mut forced: HashSet<&CheckName> = HashSet::new();
    let mut candidates: Vec<GateCheck> = Vec::new();
    for check in recheck {
        figures.forced += 1;
        figures.forced_billions += record.get(&check.check).map_or(0.0, |c| cost(c));
    }
    for check in to_run {
        match record.get(&check.check) {
            Some(scored) if scored.regression.is_none() => {
                // Builds are outside the budget, and the always set has no special place here.
                candidates.push(GateCheck {
                    always: false,
                    edited: false,
                    cached: false,
                    needs: Vec::new(),
                    ..(*scored).clone()
                });
            }
            found => {
                figures.forced += 1;
                figures.forced_billions += found.map_or(0.0, |c| cost(c));
                forced.insert(&check.check);
            }
        }
    }
    let unbuilt = std::collections::BTreeSet::new();
    let paid = std::collections::BTreeSet::new();
    let plan = gate::plan(
        &GateInput {
            areas,
            checks: &candidates,
            unbuilt: &unbuilt,
            landing: None,
            paid: &paid,
            billions_per_cpu_second: BILLIONS_PER_CPU_SECOND,
        },
        budget,
    );
    figures.within = u32::try_from(plan.run.len()).unwrap_or(u32::MAX);
    figures.within_billions = plan.scored_cost;
    let after_names: HashSet<&CheckName> = plan
        .past_budget
        .iter()
        .chain(&plan.tip)
        .map(|left| &left.check)
        .collect();
    figures.after = u32::try_from(after_names.len()).unwrap_or(u32::MAX);
    figures.after_billions = gate::cost_of(&plan.past_budget) + gate::cost_of(&plan.tip);
    let (after, within): (Vec<&TipCheck>, Vec<&TipCheck>) = to_run
        .iter()
        .copied()
        .partition(|check| !forced.contains(&check.check) && after_names.contains(&check.check));
    // The checks past the budget count as unrun until they run after `master` moves; a round
    // that does not move `master` leaves them for a later round.
    figures.unrun = unrun(after.iter(), &[]);
    Split {
        after_keys: after.iter().map(|c| c.key.clone()).collect(),
        within,
        after,
        budget: Some(figures),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ignite_core::gate::History;
    use ignite_core::ids::TicketId;
    use ignite_core::score::{AreaRecord, Cost};

    use super::*;

    fn tip_check(name: &str) -> TipCheck {
        let check = CheckName::new(name).unwrap();
        TipCheck {
            key: ResultKey {
                check: check.clone(),
                hash: ignite_core::ids::InputHash::new("a".repeat(64)).unwrap(),
                platform: None,
            },
            family: check.family().to_owned(),
            check,
            inputs: Vec::new(),
            runnable: true,
            reuse: true,
        }
    }

    fn scored(name: &str, failures: u32, cost: f64) -> GateCheck {
        GateCheck {
            family_startup: ignite_core::score::Cost::Billions(0.0),
            check: CheckName::new(name).unwrap(),
            always: false,
            edited: false,
            cached: false,
            history: Some(History {
                areas: BTreeMap::from([("kernel".to_owned(), AreaRecord { runs: 20, failures })]),
                median_cost: Cost::Billions(cost),
            }),
            family_rate: 0.01,
            family_median_cost: Cost::Billions(1.0),
            needs: vec!["rust-tests".to_owned()],
            group: None,
            regression: None,
        }
    }

    /// The likeliest checks fill the 300 s budget, a check with an open regression runs before
    /// `master` whatever the budget, and the rest wait until after it moves.
    #[test]
    fn the_best_checks_fill_the_budget_and_an_open_regression_is_forced() {
        let checks: Vec<TipCheck> = ["unit:a", "unit:b", "unit:c", "unit:d"]
            .into_iter()
            .map(tip_check)
            .collect();
        let to_run: Vec<&TipCheck> = checks.iter().collect();
        let mut broken = scored("unit:d", 0, 50.0);
        broken.regression = Some(TicketId::new("Tabc1").unwrap());
        let records = [
            scored("unit:a", 9, 100.0),
            scored("unit:b", 5, 100.0),
            scored("unit:c", 1, 100.0),
            broken,
        ];
        // 200 s at one billion a second: two of the three scored checks fit.
        let budget = Budget {
            threshold: 0.0,
            seconds: 200.0,
            rate: 1.0,
        };
        let split = split_by(&to_run, &[], &["kernel".to_owned()], &records, &budget);

        let names = |list: &[&TipCheck]| -> Vec<String> {
            list.iter().map(|c| c.check.to_string()).collect()
        };
        assert_eq!(names(&split.within), ["unit:a", "unit:b", "unit:d"]);
        assert_eq!(names(&split.after), ["unit:c"]);
        assert!(split.is_after(&checks[2]) && !split.is_after(&checks[0]));
        let figures = split.budget.unwrap();
        assert_eq!((figures.within, figures.forced, figures.after), (2, 1, 1));
        assert!((figures.within_billions - 200.0).abs() < 1e-9);
        assert!((figures.forced_billions - 50.0).abs() < 1e-9);
        assert!((figures.after_billions - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_check_without_a_record_runs_before_master() {
        let checks = [tip_check("unit:new")];
        let to_run: Vec<&TipCheck> = checks.iter().collect();
        let budget = Budget {
            threshold: 0.0,
            seconds: 0.001,
            rate: 1.0,
        };
        let split = split_by(&to_run, &[], &[], &[], &budget);
        assert_eq!(split.within.len(), 1);
        assert!(split.after.is_empty());
    }
}
