//! The landing gate: which checks a change can affect, listed and run from the cache where
//! they can be, stopping at the first fault (see "Landing" in `docs/ignite.md`).
//!
//! The gate spends the first budget, `[gate] seconds` of running checks (see "Where regression
//! checks run"). The budget belongs to the change: a gate after one that passed for the same
//! change charges it again for the checks that gate chose, so `ignite land` after
//! `ignite check` runs no second slice of checks, and its seconds convert at the rate the
//! gates of the last hour achieved (see [`super::spent`]). It builds no product its tree has never built for a check chosen by score: a
//! family whose listing needs such a product, and that has no kept listing, is not listed at
//! all unless it is in the always set or holds the check of the regression being landed, and
//! its checks are left to tip verification. The output prints the seconds spent building and
//! the seconds spent running checks on lines of their own.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

use ignite_core::budget::billions_text;
use ignite_core::gate::cost_of;
use ignite_core::ids::{CheckName, TicketId, TreeId};
use ignite_core::manifest::{Family, Kind};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::CachedResult;

use super::plan;
use super::time_lines::{Stopwatch, listed_line, slow_line, took_line};
use super::verbs::{changed_paths, counts, failure_lines, gate_view_of, open_regressions, unbuilt};
use super::{CheckRun, Checks, Keyed, OpenTree, RunOptions};
use crate::server::Context;

/// A check the gate chose, with its cached result when the plan takes it from the cache.
type Item = (Keyed, Option<CachedResult>);

/// What the landing gate ran: the checks it chose, and how many it left to tip verification.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GateRun {
    pub run: CheckRun,
    /// Checks past the budget, needing a product the tree has never built, or below the
    /// threshold, which tip verification runs.
    pub left: usize,
    /// How many of [`GateRun::left`] each family has, for the families with any. A family in
    /// `run.cold_families` is not here: it was never listed, so its checks are not counted.
    pub left_by_family: BTreeMap<String, usize>,
    /// How the gate spent its budget; `None` when it stopped before choosing.
    pub budget: Option<GateBudget>,
    /// The families the change can affect, which the gate judged (see [`Checks::scope`]),
    /// including those it carried over from an earlier pass.
    pub scoped: Vec<String>,
    /// The families an earlier passing gate of the same landing judged, and that the commits
    /// landed since touch none of the inputs of, so they were not judged again (see
    /// [`Carried`]).
    pub carried: Vec<String>,
    /// Checks the gate left out because they already fail at the tip, each with its regression
    /// ticket: those the hub has an open regression ticket for, and those the landing found
    /// failing at the tip it was rebased onto (see [`Excused`]).
    pub known_failing: Vec<(CheckName, TicketId)>,
    /// What comparing the gate's failures with the tip the landing was rebased onto found, when
    /// they still refuse the landing.
    pub base: Option<String>,
    /// The seconds the gate took, from opening the tree to its last check, which the `took`
    /// line splits into the stages recorded in `run.stages`.
    pub seconds: f64,
}

/// Checks a landing found failing at the tip it was rebased onto as well as in its own commit,
/// each with the regression ticket the hub filed or joined for it. The gate leaves them out as
/// known to fail, as it does a check with an open regression ticket.
pub type Excused = BTreeMap<CheckName, TicketId>;

/// What an earlier gate of the same landing, which passed, lets a later pass skip.
///
/// A landing that lost the race for the tip rebases onto the new tip and gates again. The
/// rebased commit differs from the one that passed only in `moved`, the files the commits
/// landed since changed, so a family the change could affect, and whose inputs `moved` touches
/// none of, would read exactly what it read before. Such a family is carried over instead of
/// being listed, hashed and looked up again; when every family is, the gate runs nothing and
/// the landing pushes at once. The landing passes this only when the change's own files are
/// the same as in the pass that passed, because the scope is judged from them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Carried {
    /// The families the passing gate judged: its [`GateRun::scoped`].
    pub families: Vec<String>,
    /// The paths that differ between the commit whose gate passed and the commit gated now.
    pub moved: Vec<String>,
}

/// How the gate spent its budget, in billions of instructions from each check's recorded cost.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GateBudget {
    /// `[gate] seconds`.
    pub seconds: f64,
    /// Billions of instructions per second of wall time that seconds converted at.
    pub rate: f64,
    /// Chosen by score and run, with the start-ups of the families they start.
    pub scored: (usize, f64),
    /// Run whatever the budget: the always set, edited checks and the regression being fixed,
    /// with the start-ups of their families.
    pub forced: (usize, f64),
    /// Answered from the cache, with what running them would have cost.
    pub cached: (usize, f64),
    /// Billions of the budget an earlier gate of the same change spent on checks this
    /// gate answered from the cache (see `ignite_core::gate`).
    pub paid: f64,
    /// Left to tip verification because the budget was spent.
    pub past_budget: (usize, f64),
    /// Left to tip verification because they need a product the tree has never built.
    pub cold: (usize, f64),
    /// Left to tip verification because their score is below the threshold.
    pub below: (usize, f64),
    /// The products the tree had never built when the gate started.
    pub unbuilt: BTreeSet<String>,
}

impl GateBudget {
    /// The budget in billions of instructions.
    pub fn billions(&self) -> f64 {
        self.seconds * self.rate
    }
}

impl Checks {
    /// Runs what the landing gate runs on `tree` as it is: the always set and the checks whose
    /// score passes the threshold, within the cap, each from the cache where it can be.
    pub fn gate(&self, ctx: &Context, tree: &TreeId, options: &RunOptions) -> GateRun {
        self.gate_carrying(ctx, tree, options, None, &Excused::new())
    }

    /// [`Checks::gate`], judging again only the families `carried` does not carry over, and
    /// leaving out the checks in `excused` as known to fail.
    pub fn gate_carrying(
        &self,
        ctx: &Context,
        tree: &TreeId,
        options: &RunOptions,
        carried: Option<&Carried>,
        excused: &Excused,
    ) -> GateRun {
        let opening = Stopwatch::start(ctx);
        match self.open(ctx, tree) {
            Ok((checks, tree)) => {
                let mut gated = checks.gate_in(ctx, &tree, options, carried, excused);
                gated.opened(ctx, opening);
                gated
            }
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

    pub(super) fn gate_in(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        options: &RunOptions,
        carried: Option<&Carried>,
        excused: &Excused,
    ) -> GateRun {
        let started = Stopwatch::start(ctx);
        let mut gated = self.gate_stages(ctx, tree, options, carried, excused);
        gated.seconds = started.seconds(ctx);
        gated
    }

    /// [`Checks::gate_in`], recording each stage it reaches in `run.stages`.
    fn gate_stages(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        options: &RunOptions,
        carried: Option<&Carried>,
        excused: &Excused,
    ) -> GateRun {
        let mut run = CheckRun::default();
        let scoping = Stopwatch::start(ctx);
        // Without the changed paths the gate cannot tell which checks the change edited, and
        // would drop them silently, so it fails instead.
        let changed = match changed_paths(self, ctx, tree) {
            Ok(changed) => changed,
            Err(error) => {
                run.incomplete_families.push((
                    format!("the changed files of the tree cannot be read: {error}"),
                    std::path::PathBuf::new(),
                ));
                return GateRun {
                    run,
                    ..GateRun::default()
                };
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
        let scoped: Vec<String> = among.iter().map(|(name, _)| (*name).to_owned()).collect();
        let (among, carried) = match carried {
            Some(carried) => self.carry(ctx, tree, among, carried),
            None => (among, Vec::new()),
        };
        run.stage(ctx, "finding the scope", scoping);
        // Nothing the commits landed since touch can change a verdict, so nothing is listed,
        // built or run.
        if among.is_empty() && !carried.is_empty() {
            return GateRun {
                run,
                scoped,
                carried,
                ..GateRun::default()
            };
        }
        // A family listed even if that builds a product the tree has never built: one in the
        // always set for this change, or one holding the check whose regression this fixes.
        let listing = Stopwatch::start(ctx);
        let fixing = fixing(ctx, options);
        let unbuilt = unbuilt(ctx, tree, among.iter().map(|(_, family)| *family));
        let listed = |name: &str, f: &Family| f.always_for(&changed) || fixing.contains(name);
        let mut prepared =
            self.prepare_gate(ctx, tree, &among, options, &mut run, &|name, family| {
                let needs = family.list_needs.as_ref().unwrap_or(&family.needs);
                needs.iter().any(|need| unbuilt.contains(need)) && !listed(name, family)
            });
        for family in &mut prepared {
            family.keep(|_| true, &mut run);
        }
        run.stage(ctx, "listing", listing);
        // A fault found while listing, such as an invalid manifest entry, refuses the landing
        // before anything is looked up, built or run.
        if !run.passed() {
            return GateRun {
                run,
                ..GateRun::default()
            };
        }
        let planning = Stopwatch::start(ctx);
        let hits = self.lookup(&prepared, options);
        let mut view = gate_view_of(self, ctx, tree, &prepared, &hits, &changed);
        landing_view(&mut view, options, excused);
        let gate_plan = self.remember_choice(ctx, &changed, view.plan(&ctx.config.gate));
        // The products never built before the gate started, so the `build` line can name a
        // cold build even when listing a family made the product.
        let budget = gate_budget(&view.budget(&ctx.config.gate), &gate_plan, unbuilt.clone());
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
        run.stage(ctx, "planning", planning);
        let running = Stopwatch::start(ctx);
        self.run_families(ctx, tree, families, options, &mut run);
        run.stage(ctx, "running", running);
        self.remember_rate(ctx, &gate_plan, &run, running.seconds(ctx));
        GateRun {
            run,
            left: gate_plan.past_budget.len() + gate_plan.cold.len() + gate_plan.tip.len(),
            left_by_family: left_by_family(&gate_plan),
            budget: Some(budget),
            scoped,
            carried,
            known_failing: gate_plan.known_failing,
            base: None,
            seconds: 0.0,
        }
    }

    /// Runs every family at once, each on a thread of its own, started in the order given,
    /// which ranks them by the failures they are likely to find first. Once a failure is
    /// confirmed, the landing is refused: no further command starts in any family, and what
    /// already passed stays in the cache. What each family found is added to `run` in that
    /// order. On 2026-11-19 the gate of a one-card comment change ran its families one after
    /// another for 123 s, though no single family took more than 41 s.
    fn run_families(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: Vec<(String, Vec<Item>)>,
        options: &RunOptions,
        run: &mut CheckRun,
    ) {
        let stop = AtomicBool::new(false);
        let ran = super::prepare::on_threads(families, |(family, items), own| {
            self.execute(ctx, tree, &family, items, options, own, Some(&stop));
        });
        for ((), own) in ran {
            run.absorb(own);
        }
    }

    /// Remembers what `gate_plan` chose by score, before any of it runs, so the change's next
    /// gate spends no second budget (see `spent`), and returns the plan.
    ///
    /// The choice is remembered whatever becomes of the run. It was once remembered only when
    /// the gate passed, and on 2026-11-20 neither the two `ignite check`s of a one-card comment
    /// change nor its first landing passed, because a file was missing from the machine's cache.
    /// The landing was cut off by daemon restarts and run again five times in 13 minutes, and
    /// each run found the checks the run before had finished in the cache, where they cost
    /// nothing, and spent a fresh budget on the next checks by score: about 2,100 checks and two
    /// builds of a product the change does not touch. The landing after it ran 1,685 more
    /// checks for the same reason.
    fn remember_choice(
        &self,
        ctx: &Context,
        changed: &[String],
        gate_plan: ignite_core::gate::GatePlan,
    ) -> ignite_core::gate::GatePlan {
        use ignite_core::gate::Why;
        let chosen: BTreeSet<CheckName> = gate_plan
            .run
            .iter()
            .chain(&gate_plan.cached)
            .filter(|planned| planned.why == Why::Scored)
            .map(|planned| planned.check.clone())
            .collect();
        let conn = self.cache.conn();
        super::spent::GateMemory(&conn).chose(ctx.clock.now(), changed, &chosen);
        gate_plan
    }

    /// Remembers the rate the gate's check commands achieved over `running_s`, the seconds the
    /// running stage took, so the next budget's seconds convert at the load of the moment (see
    /// `spent`).
    ///
    /// The rate is taken from every gate that ran all it chose, which is one where no check
    /// failed, since a failure stops the rest. On 2026-11-19 two gates of a one-card change
    /// on one machine both reported a check incomplete because a file was missing from the
    /// machine's cache, so neither recorded its rate; the last one went stale, and the second
    /// gate planned at the hub's daily rate, 5.7 billion a second, and ran 232 s against a 60 s
    /// budget. The seconds are those that passed while the families ran at once, not the sum
    /// of their commands' seconds, which was 512 s in that gate, because the budget is
    /// seconds of waiting.
    fn remember_rate(
        &self,
        ctx: &Context,
        gate_plan: &ignite_core::gate::GatePlan,
        run: &CheckRun,
        running_s: f64,
    ) {
        let ran_all = run.build_failures.is_empty()
            && run.skipped == 0
            && !run.outcomes.iter().any(|(_, o)| *o == Outcome::Fail);
        if ran_all {
            let conn = self.cache.conn();
            let billions = gate_plan.scored_cost + gate_plan.forced_cost;
            super::spent::GateMemory(&conn).observe(ctx.clock.now(), billions, running_s);
        }
    }

    /// Splits the families the change can affect into those to judge again and the names of
    /// those `carried` carries over: a family the earlier gate judged, whose inputs the files
    /// in `carried.moved` touch none of. A family the earlier gate did not judge, such as one
    /// the commits landed since declared, is always judged.
    fn carry<'a>(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: Vec<(&'a str, &'a Family)>,
        carried: &Carried,
    ) -> (Vec<(&'a str, &'a Family)>, Vec<String>) {
        let (judged, new): (Vec<_>, Vec<_>) = among
            .iter()
            .copied()
            .partition(|(name, _)| carried.families.iter().any(|f| f == name));
        let (touched, untouched) = self.scope(ctx, tree, &judged, &carried.moved);
        let again = among
            .into_iter()
            .filter(|(name, _)| {
                new.iter().any(|(n, _)| n == name) || touched.iter().any(|(n, _)| n == name)
            })
            .collect();
        (again, untouched)
    }
}

impl GateRun {
    /// Counts the seconds since `opening`, when the gate began opening its tree, as the gate's
    /// first stage and in its total.
    pub(crate) fn opened(&mut self, ctx: &Context, opening: Stopwatch) {
        let total = opening.seconds(ctx);
        let opened = (total - self.seconds).max(0.0);
        self.run
            .stages
            .insert(0, ("opening the tree".to_owned(), opened));
        self.seconds = total;
    }
}

/// The families holding a check whose open regression ticket is the one being landed.
fn fixing(ctx: &Context, options: &RunOptions) -> BTreeSet<String> {
    open_regressions(ctx)
        .into_iter()
        .filter(|(_, ticket)| options.ticket.as_ref() == Some(ticket))
        .map(|(check, _)| check.family().to_owned())
        .collect()
}

/// Names the ticket being landed in `view`, and gives each check in `excused` its regression
/// ticket, so the plan leaves it out as known to fail.
fn landing_view(view: &mut plan::GateView, options: &RunOptions, excused: &Excused) {
    view.landing.clone_from(&options.ticket);
    for check in &mut view.checks {
        if check.regression.is_none() {
            check.regression = excused.get(&check.check).cloned();
        }
    }
}

/// How many checks of each family `plan` leaves to tip verification.
fn left_by_family(plan: &ignite_core::gate::GatePlan) -> BTreeMap<String, usize> {
    let mut left = BTreeMap::new();
    for check in plan.past_budget.iter().chain(&plan.cold).chain(&plan.tip) {
        *left.entry(check.check.family().to_owned()).or_insert(0) += 1;
    }
    left
}

/// The counts and costs of each part of `plan`.
fn gate_budget(
    budget: &ignite_core::budget::Budget,
    plan: &ignite_core::gate::GatePlan,
    unbuilt: BTreeSet<String>,
) -> GateBudget {
    use ignite_core::gate::{Planned, Why};
    let part = |planned: &[&Planned]| (planned.len(), planned.iter().map(|p| p.cost).sum::<f64>());
    let (scored, forced): (Vec<&Planned>, Vec<&Planned>) =
        plan.run.iter().partition(|p| p.why == Why::Scored);
    let cached: Vec<&Planned> = plan.cached.iter().collect();
    GateBudget {
        seconds: budget.seconds,
        rate: budget.rate,
        // What the run checks spend includes the start-ups of the families they start.
        scored: (scored.len(), plan.scored_cost),
        forced: (forced.len(), plan.forced_cost),
        cached: part(&cached),
        paid: plan.paid_cost,
        past_budget: (plan.past_budget.len(), cost_of(&plan.past_budget)),
        cold: (plan.cold.len(), cost_of(&plan.cold)),
        below: (plan.tip.len(), cost_of(&plan.tip)),
        unbuilt,
    }
}

/// The `gate`, `budget`, `build`, `listed`, `slow` and `took` lines, the failures and the `tip`
/// line of a gate's output.
/// Check seconds and build seconds are on lines of their own, so a landing that was slow
/// because it compiled shows as that.
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
            "{total} checks: {} from cache, {} run, {verdict} in {:.0} s of running checks",
            run.from_cache, run.ran, run.wall_s
        ),
    )];
    if let Some(budget) = &gated.budget {
        lines.push(budget_line(budget));
    }
    lines.push(build_line(run, gated.budget.as_ref()));
    lines.extend(listed_line(run));
    lines.extend(slow_line(run));
    lines.extend(took_line(run, gated.seconds));
    lines.extend(failure_lines(run));
    lines.extend(gated.base.clone().map(|base| ("base".to_owned(), base)));
    lines.extend(stopped_line(run));
    lines.extend(scope_line(run));
    lines.extend(carried_line(gated));
    lines.extend(known_line(gated));
    lines.extend(tip_line(gated));
    lines
}

/// The `known` line: the checks left out because they already fail at the tip, with their
/// regression tickets.
fn known_line(gated: &GateRun) -> Option<(String, String)> {
    let n = gated.known_failing.len();
    let checks = if n == 1 { "check" } else { "checks" };
    (n > 0).then(|| {
        let named: Vec<String> = gated
            .known_failing
            .iter()
            .map(|(check, ticket)| format!("{check} ({ticket})"))
            .collect();
        (
            "known".to_owned(),
            format!(
                "{n} {checks} left out as known-failing: they fail at the tip too: {}",
                named.join(", ")
            ),
        )
    })
}

/// The `carried` line: the families an earlier pass of the landing judged, which this pass
/// did not judge again because the commits landed since touch none of their inputs.
fn carried_line(gated: &GateRun) -> Option<(String, String)> {
    let n = gated.carried.len();
    let families = if n == 1 { "family" } else { "families" };
    (n > 0).then(|| {
        (
            "carried".to_owned(),
            format!(
                "{n} {families} from the earlier pass: the commits landed since touch none of \
                 their inputs"
            ),
        )
    })
}

/// `(3, 1204.4)` as `3 checks (1,204 billion)`.
fn part_text((count, billions): (usize, f64)) -> String {
    let word = if count == 1 { "check" } else { "checks" };
    format!(
        "{} {word} ({} billion)",
        plan::thousands(count),
        billions_text(billions)
    )
}

/// The `budget` line: the budget, the rate it converted at, and what was spent within it,
/// including what an earlier gate of the change spent on checks now cached.
fn budget_line(budget: &GateBudget) -> (String, String) {
    let paid = if budget.paid > 0.0 {
        format!(
            ", {} billion spent by an earlier gate of this change",
            billions_text(budget.paid)
        )
    } else {
        String::new()
    };
    (
        "budget".to_owned(),
        format!(
            "{:.0} s at {:.1} billion/s = {} billion: {} within it, {} outside it, {} \
             cached{paid}",
            budget.seconds,
            budget.rate,
            billions_text(budget.billions()),
            part_text(budget.scored),
            part_text(budget.forced),
            part_text(budget.cached),
        ),
    )
}

/// The `build` line: the seconds spent making products current, apart from running checks,
/// and the products the tree had never built.
fn build_line(run: &CheckRun, budget: Option<&GateBudget>) -> (String, String) {
    if run.builds.is_empty() {
        return ("build".to_owned(), "nothing built".to_owned());
    }
    let seconds: f64 = run.builds.iter().map(|b| b.wall_s).sum();
    let count = run.builds.len();
    let word = if count == 1 { "product" } else { "products" };
    let cold: Vec<&str> = run
        .builds
        .iter()
        .map(|b| b.product.as_str())
        .filter(|product| budget.is_some_and(|b| b.unbuilt.contains(*product)))
        .collect();
    let cold = if cold.is_empty() {
        String::new()
    } else {
        format!("; never built here before: {}", cold.join(", "))
    };
    (
        "build".to_owned(),
        format!("{seconds:.0} s building {count} {word}{cold}"),
    )
}

/// The `tip` line: what the gate left to tip verification, and why.
fn tip_line(gated: &GateRun) -> Option<(String, String)> {
    let families = gated.run.cold_families.len();
    if gated.left == 0 && families == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(budget) = &gated.budget {
        for (part, why) in [
            (budget.past_budget, "past the budget"),
            (budget.cold, "needing a product this tree has never built"),
            (budget.below, "below the threshold"),
        ] {
            if part.0 > 0 {
                parts.push(format!("{} {why}", part_text(part)));
            }
        }
    }
    if families > 0 {
        let word = if families == 1 { "family" } else { "families" };
        parts.push(format!(
            "{families} {word} not listed, as listing needs a product this tree has never \
             built ({})",
            gated.run.cold_families.join(", ")
        ));
    }
    Some((
        "tip".to_owned(),
        format!(
            "{} checks left to tip verification: {}",
            plan::thousands(gated.left),
            parts.join("; ")
        ),
    ))
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
