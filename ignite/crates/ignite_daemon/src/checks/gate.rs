//! The landing gate: which checks a change can affect, listed and run from the cache where
//! they can be, stopping at the first fault (see "Landing" in `docs/ignite.md`).

use ignite_core::ids::{CheckName, TreeId};
use ignite_core::manifest::{Family, Kind};
use ignite_core::protocol::hub::CachedResult;

use super::plan;
use super::verbs::{changed_paths, counts, failure_lines, gate_view_of};
use super::{CheckRun, Checks, Keyed, OpenTree, RunOptions};
use crate::server::Context;

/// A check the gate chose, with its cached result when the plan takes it from the cache.
type Item = (Keyed, Option<CachedResult>);

/// What the landing gate ran: the checks it chose, and how many it left to tip verification.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GateRun {
    pub run: CheckRun,
    /// Checks past the cap or below the threshold, which tip verification runs.
    pub left: usize,
}

impl Checks {
    /// Runs what the landing gate runs on `tree` as it is: the always set and the checks whose
    /// score passes the threshold, within the cap, each from the cache where it can be.
    pub fn gate(&self, ctx: &Context, tree: &TreeId, options: &RunOptions) -> GateRun {
        match self.open(tree) {
            Ok(tree) => self.gate_in(ctx, &tree, options),
            Err(error) => {
                let mut gated = GateRun::default();
                gated
                    .run
                    .incomplete_families
                    .push((format!("tree {tree}: {error}"), std::path::PathBuf::new()));
                gated
            }
        }
    }

    pub(super) fn gate_in(&self, ctx: &Context, tree: &OpenTree, options: &RunOptions) -> GateRun {
        let mut run = CheckRun::default();
        // Without the changed paths the gate cannot tell which checks the change edited, and
        // would drop them silently, so it fails instead.
        let changed = match changed_paths(self, ctx, tree) {
            Ok(changed) => changed,
            Err(error) => {
                run.incomplete_families.push((
                    format!("the changed files of the tree cannot be read: {error}"),
                    std::path::PathBuf::new(),
                ));
                return GateRun { run, left: 0 };
            }
        };
        // Which families the change can affect is decided first, so a family it cannot affect
        // is neither listed nor built.
        let regression: Vec<(&str, &Family)> = self
            .families()
            .filter(|(_, f)| f.kind == Kind::Regression)
            .collect();
        let (among, left_out) = self.scope(ctx, tree, &regression, &changed);
        run.out_of_scope = left_out;
        let mut prepared = self.prepare_gate(ctx, tree, &among, options, &mut run);
        for family in &mut prepared {
            family.keep(|_| true, &mut run);
        }
        // A fault found while listing, such as an invalid manifest entry, refuses the landing
        // before anything is looked up, built or run.
        if !run.passed() {
            return GateRun { run, left: 0 };
        }
        let hits = self.lookup(&prepared, options);
        let mut view = gate_view_of(self, ctx, tree, &prepared, &hits, &changed);
        view.landing.clone_from(&options.ticket);
        let gate_plan = view.plan(&ctx.config.gate);
        let to_run: Vec<&CheckName> = gate_plan.run.iter().map(|p| &p.check).collect();
        let cached: Vec<&CheckName> = gate_plan.cached.iter().map(|p| &p.check).collect();
        let mut hits = hits.into_iter();
        let mut families: Vec<(String, Vec<Item>)> = prepared
            .into_iter()
            .map(|family| {
                let items = family
                    .keyed
                    .into_iter()
                    .map(|k| (k, hits.next().flatten()))
                    .filter(|(k, _)| {
                        to_run.contains(&&k.listed.check) || cached.contains(&&k.listed.check)
                    })
                    .map(|(k, hit)| {
                        let hit = hit.filter(|_| cached.contains(&&k.listed.check));
                        (k, hit)
                    })
                    .collect();
                (family.family, items)
            })
            .collect();
        let order = plan::run_order(&view, &gate_plan);
        let key = |family: &str| order.get(family).copied().unwrap_or(0.0);
        families.sort_by(|a, b| key(&a.0).total_cmp(&key(&b.0)));
        for (family, items) in families {
            // Once a check has failed, the landing is refused: no further command starts, and
            // what already passed stays in the cache.
            if !run.passed() {
                run.skipped += u32::try_from(items.len()).unwrap_or(u32::MAX);
                continue;
            }
            self.execute(ctx, tree, &family, items, options, &mut run, true);
        }
        GateRun {
            run,
            left: gate_plan.past_cap.len() + gate_plan.tip.len(),
        }
    }
}

/// The `gate` line, the failures and the `tip` line of a gate's output.
pub(crate) fn gate_lines(gated: &GateRun) -> Vec<(String, String)> {
    let run = &gated.run;
    let total = run.outcomes.len();
    let verdict = if run.passed() {
        "all passed".to_owned()
    } else {
        counts(run)
    };
    let mut lines = vec![(
        "gate".to_owned(),
        format!(
            "{total} checks: {} from cache, {} run, {verdict} in {:.0} s",
            run.from_cache, run.ran, run.wall_s
        ),
    )];
    lines.extend(failure_lines(run));
    lines.extend(stopped_line(run));
    lines.extend(scope_line(run));
    if gated.left > 0 {
        lines.push((
            "tip".to_owned(),
            format!(
                "{} checks left to tip verification",
                plan::thousands(gated.left)
            ),
        ));
    }
    lines
}

/// The `stopped` line: what the gate did not list or run because it stopped at the first fault.
fn stopped_line(run: &CheckRun) -> Option<(String, String)> {
    let families = run.unlisted.len();
    let checks = run.skipped;
    let mut parts = Vec::new();
    if families > 0 {
        let word = if families == 1 { "family" } else { "families" };
        parts.push(format!("{families} {word} not listed"));
    }
    if checks > 0 {
        let word = if checks == 1 { "check" } else { "checks" };
        parts.push(format!(
            "{} {word} not run",
            plan::thousands(checks as usize)
        ));
    }
    (!parts.is_empty()).then(|| {
        (
            "stopped".to_owned(),
            format!("at the first fault: {}", parts.join(", ")),
        )
    })
}

/// The `scope` line: how many families the gate left out because the change touches none of
/// their inputs.
pub(super) fn scope_line(run: &CheckRun) -> Option<(String, String)> {
    let n = run.out_of_scope.len();
    let families = if n == 1 { "family" } else { "families" };
    (n > 0).then(|| {
        (
            "scope".to_owned(),
            format!("{n} {families} left out: the change touches none of their inputs"),
        )
    })
}
