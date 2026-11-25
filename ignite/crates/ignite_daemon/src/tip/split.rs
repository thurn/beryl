//! The tip budget: which of a round's checks run before `master` moves, and running the rest
//! afterwards (see "Tip verification" in `docs/ignite.md`).
//!
//! - The first verifier scores its remaining checks as the landing gate does and saves the
//!   split at the candidate before running them. Portable checks and that platform's checks
//!   share `[tip] seconds` at the `tip` class's rate. Other platforms reuse the portable
//!   selection and spend what is left on their native checks. Retries reuse both decisions
//!   even as cache hits arrive.
//! - Some checks run before `master` moves whatever the budget: a cached failure being run
//!   again, an always check, a check with an open regression ticket, and a check with no hub
//!   record. Portable and platform-keyed checks share the budget. A reported failure among
//!   checks past the budget holds `master` on later rounds until its fix lands.
//! - When the checks within the budget, the forced ones and every check already answered pass,
//!   `master` moves. The checks past the budget then run in the same round, in the `tip` class
//!   on whatever cores are free. A failure among them is reported as a regression, which files
//!   its ticket; `master` has already moved and stays. A newer `staging` tip ends them at the
//!   next family or batch, even with batches under way, since the tip loop starts no round
//!   until this one ends.
//! - The saved split exempts only exact result keys. Required native checks and missing
//!   listings still hold `master`. Unrun native checks past the budget leave a coverage gap
//!   so their platform can catch up after promotion.
//! - When nothing can be scored (the hub or git cannot be read), every check runs before
//!   `master` moves, as before the budget.
//! - With `repository.green` set, the checks of families declared `builds = true` (the build
//!   checks, which fail when the code does not compile or type-check) are a group of their own,
//!   outside the budget, which the round runs before any other check (see `green`).

use std::collections::{BTreeMap, HashMap, HashSet};

use ignite_core::budget::Budget;
use ignite_core::config::Os;
use ignite_core::gate::{self, GateCheck, GateInput};
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::score::BILLIONS_PER_CPU_SECOND;
use ignite_core::tipround::{PortableTipBudget, RoundBudget, TipSelection, TipSelectionUpdate};
use ignite_core::traits::Result;

use super::coverage::Scope;
use super::families::{Elsewhere, RunPhase};
use super::meter::Tally;
use super::verify::RunPlan;
use super::{TipCheck, TipRun, TipVerifier, candidate, settle_runs};
use crate::checks::plan::class_rates;
use crate::server::Context;

/// A round's checks to run, split by the tip budget.
pub(super) struct Split<'a> {
    /// The checks to run again, ignoring their cached results, before `master` moves.
    pub recheck: Vec<&'a TipCheck>,
    /// The checks to run before `master` moves: forced and within the budget.
    pub within: Vec<&'a TipCheck>,
    /// The checks to run after `master` moves.
    pub after: Vec<&'a TipCheck>,
    /// The split's figures; `None` when nothing could be scored.
    pub budget: Option<RoundBudget>,
    after_keys: HashSet<ResultKey>,
    portable_billions: f64,
}

impl<'a> Split<'a> {
    fn unbudgeted(queued: &[&'a TipCheck], recheck: &[&'a TipCheck]) -> Self {
        Self {
            recheck: recheck.to_vec(),
            within: queued.to_vec(),
            after: Vec::new(),
            budget: None,
            after_keys: HashSet::new(),
            portable_billions: 0.0,
        }
    }

    fn reuse(
        plan: &RunPlan<'a>,
        saved: &TipSelection,
        platform: Option<Os>,
        regressions: &HashSet<CheckName>,
    ) -> Self {
        let mut after_keys: HashSet<_> = saved
            .portable
            .after
            .iter()
            .chain(
                platform
                    .and_then(|os| saved.native.get(&os))
                    .into_iter()
                    .flatten(),
            )
            .cloned()
            .collect();
        for check in &plan.recheck {
            after_keys.remove(&check.key);
        }
        let (after, within): (Vec<_>, Vec<_>) = plan.queued.iter().copied().partition(|check| {
            after_keys.contains(&check.key) && budget_eligible(check, regressions)
        });
        let budget = RoundBudget {
            reused: true,
            within: u32::try_from(within.len()).unwrap_or(u32::MAX),
            forced: u32::try_from(plan.recheck.len()).unwrap_or(u32::MAX),
            after: u32::try_from(after.len()).unwrap_or(u32::MAX),
            unrun: unrun(after.iter(), &[], &Elsewhere::new()),
            ..RoundBudget::default()
        };
        Self {
            recheck: plan.recheck.clone(),
            within,
            after,
            budget: Some(budget),
            after_keys,
            portable_billions: saved.portable.billions,
        }
    }

    /// Whether `check` waits until after `master` moves.
    pub fn is_after(&self, check: &TipCheck) -> bool {
        self.after_keys.contains(&check.key)
    }
}

impl TipVerifier {
    /// Keeps the candidate's selection fixed even when its selected verdicts arrive on
    /// different rounds. A racing proposal has to reuse the winning portable decision.
    pub(super) fn split<'a>(
        &self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        plan: &RunPlan<'a>,
        scope: &Scope<'_>,
        regressions: &HashSet<CheckName>,
    ) -> Result<Split<'a>> {
        if scope.catch_up_only() {
            return Ok(Split::unbudgeted(&plan.queued, &plan.recheck));
        }
        loop {
            let saved = ctx.hub.tip_selection(tip)?;
            if let Some(saved) = &saved
                && scope.local.is_none_or(|os| saved.native.contains_key(&os))
            {
                return Ok(Split::reuse(plan, saved, scope.local, regressions));
            }
            let local = |check: &&TipCheck| saved.is_none() || check.key.platform == scope.local;
            let queued: Vec<_> = plan.queued.iter().copied().filter(local).collect();
            let recheck: Vec<_> = plan.recheck.iter().copied().filter(local).collect();
            let asked: Vec<_> = queued.iter().chain(&recheck).copied().collect();
            let mut split = Split::unbudgeted(&queued, &recheck);
            if !asked.is_empty() {
                let Some(scoring) = self.work.scoring(ctx, last, tip, &asked) else {
                    // A scoring error forces the new work for this round. A selection the hub
                    // already fixed still applies to the portable checks.
                    return Ok(saved.as_ref().map_or(split, |saved| {
                        Split::reuse(plan, saved, scope.local, regressions)
                    }));
                };
                let rate = class_rates(ctx).tip();
                let portable = saved.as_ref().map_or(0.0, |saved| saved.portable.billions);
                let budget = Budget {
                    threshold: 0.0,
                    seconds: (ctx.config.tip.seconds - portable / rate).max(0.0),
                    rate,
                };
                split = split_by(
                    &queued,
                    &recheck,
                    &scoring.areas,
                    &scoring.checks,
                    &budget,
                    regressions,
                );
            }
            let update = TipSelectionUpdate {
                commit: tip.clone(),
                platform: scope.local,
                portable: saved.is_none().then(|| PortableTipBudget {
                    after: split
                        .after_keys
                        .iter()
                        .filter(|key| key.platform.is_none())
                        .cloned()
                        .collect(),
                    billions: split.portable_billions,
                }),
                native_after: split
                    .after_keys
                    .iter()
                    .filter(|key| key.platform.is_some() && key.platform == scope.local)
                    .cloned()
                    .collect(),
            };
            match ctx.hub.update_tip_selection(&update)? {
                None => return Ok(split), // An older hub still uses the local split.
                Some(true) => {
                    return Ok(saved.map_or(split, |saved| {
                        Split::reuse(
                            plan,
                            &update.apply(Some(saved)).expect("new platform"),
                            scope.local,
                            regressions,
                        )
                    }));
                }
                Some(false) if candidate::replacement(ctx, tip).is_some() => {
                    return Ok(Split::unbudgeted(&plan.queued, &plan.recheck));
                }
                Some(false) => {} // Read the winning partition and use it.
            }
        }
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
        scope: &super::coverage::Scope<'_>,
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
        let (superseded, runs, elsewhere) =
            self.run_families(ctx, tip, &split.after, RunPhase::AfterMaster)?;
        // Until now every check past the budget counted as unrun (see `split_by`).
        let left = unrun(split.after.iter(), &runs, &elsewhere);
        note_elsewhere(tally, &elsewhere);
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
            let prior = scope.last_good_for(check);
            if let Err(error) = self.blame(
                ctx,
                prior.unwrap_or(last),
                tip,
                check,
                output.as_deref(),
                false,
            ) {
                eprintln!(
                    "ignite: regression of {} not reported: {error}",
                    check.check
                );
            }
        }
        Ok(())
    }
}

/// These checks cannot move after promotion, even if their keys are in a saved split.
pub(super) fn budget_eligible(check: &TipCheck, regressions: &HashSet<CheckName>) -> bool {
    check.reuse && !check.is_build() && !regressions.contains(&check.check)
}

/// For each family, how many of `set` have no run among `runs` and were not left to other
/// machines in `elsewhere`.
pub(super) fn unrun<'a>(
    set: impl Iterator<Item = &'a &'a TipCheck>,
    runs: &[TipRun],
    elsewhere: &Elsewhere,
) -> BTreeMap<String, u32> {
    let ran: HashSet<&CheckName> = runs
        .iter()
        .map(|run| &run.check)
        .chain(elsewhere.values().flatten())
        .collect();
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

/// Counts in the round's tally the checks it left to other machines, by family.
pub(super) fn note_elsewhere(tally: &mut Tally, elsewhere: &Elsewhere) {
    for (family, names) in elsewhere {
        let count = u32::try_from(names.len()).unwrap_or(u32::MAX);
        let total = tally.elsewhere.entry(family.clone()).or_default();
        *total = total.saturating_add(count);
    }
}

/// The split of `to_run` by `budget`, with `scored` the hub's record of the checks over `areas`.
/// A check with no record (switched off since it was listed, say) runs before `master` moves.
/// Build, always and open regression checks also run whatever the budget.
fn split_by<'a>(
    to_run: &[&'a TipCheck],
    recheck: &[&'a TipCheck],
    areas: &[String],
    scored: &[GateCheck],
    budget: &Budget,
    regressions: &HashSet<CheckName>,
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
    let mut portable: HashSet<&CheckName> = HashSet::new();
    let mut candidates: Vec<GateCheck> = Vec::new();
    for check in recheck {
        figures.forced += 1;
        figures.forced_billions += record.get(&check.check).map_or(0.0, |c| cost(c));
    }
    for check in to_run {
        match record.get(&check.check) {
            Some(scored)
                if budget_eligible(check, regressions)
                    && !scored.always
                    && scored.regression.is_none() =>
            {
                if check.key.platform.is_none() {
                    portable.insert(&check.check);
                }
                candidates.push(GateCheck {
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
            deleted_inputs: &std::collections::BTreeSet::new(),
            recent_product_catches: &std::collections::BTreeSet::new(),
            paid: &paid,
            billions_per_cpu_second: BILLIONS_PER_CPU_SECOND,
        },
        budget,
    );
    figures.within = u32::try_from(plan.run.len()).unwrap_or(u32::MAX);
    figures.within_billions = plan.scored_cost;
    let mut portable_families = HashSet::new();
    let portable_billions = plan
        .run
        .iter()
        .filter(|check| portable.contains(&check.check))
        .map(|check| {
            let startup = if portable_families.insert(check.check.family()) {
                record[&check.check]
                    .family_startup
                    .billions(BILLIONS_PER_CPU_SECOND)
            } else {
                0.0
            };
            check.cost + startup
        })
        .sum();
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
    figures.unrun = unrun(after.iter(), &[], &Elsewhere::new());
    Split {
        after_keys: after.iter().map(|c| c.key.clone()).collect(),
        recheck: recheck.to_vec(),
        within,
        after,
        budget: Some(figures),
        portable_billions,
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
            machine_relative: false,
            kind: crate::tip::TipCheckKind::Other,
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
        let split = split_by(
            &to_run,
            &[],
            &["kernel".to_owned()],
            &records,
            &budget,
            &HashSet::new(),
        );

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
    fn twenty_thousand_platform_checks_share_the_budget_with_portable_checks() {
        let mut checks: Vec<_> = (0..20_000)
            .map(|index| {
                let mut check = tip_check(&format!("native:{index:05}"));
                check.key.platform = Some(ignite_core::config::Os::Windows);
                check
            })
            .collect();
        checks.push(tip_check("portable:a"));
        let mut records: Vec<_> = checks
            .iter()
            .map(|check| scored(check.check.as_str(), 1, 1.0))
            .collect();
        records[0].regression = Some(TicketId::new("Tabc1").unwrap());
        records
            .last_mut()
            .unwrap()
            .history
            .as_mut()
            .unwrap()
            .areas
            .get_mut("kernel")
            .unwrap()
            .failures = 9;
        let recheck = &checks[1..2];
        let to_run: Vec<_> = checks
            .iter()
            .filter(|check| check.key != recheck[0].key)
            .collect();
        let budget = Budget {
            threshold: 0.0,
            seconds: 300.0,
            rate: 1.0,
        };

        let split = split_by(
            &to_run,
            &[&recheck[0]],
            &["kernel".to_owned()],
            &records,
            &budget,
            &HashSet::new(),
        );
        let figures = split.budget.as_ref().unwrap();

        assert_eq!(figures.forced, 2);
        assert_eq!(figures.within, 300);
        assert!((figures.within_billions - 300.0).abs() < 1e-9);
        assert_eq!(figures.after, 19_699);
        assert_eq!(split.recheck, [&checks[1]]);
        assert!(split.within.contains(&&checks[0]));
        assert!(split.within.contains(&checks.last().unwrap()));
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
        let split = split_by(&to_run, &[], &[], &[], &budget, &HashSet::new());
        assert_eq!(split.within.len(), 1);
        assert!(split.after.is_empty());
    }
}
