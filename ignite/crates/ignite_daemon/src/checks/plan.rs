//! From listed checks and the hub's record of them to the inputs of [`gate::plan`], and from a
//! [`GatePlan`] to the text of `ignite check --plan` (see "Where regression checks run" in
//! `docs/ignite.md`).

use std::collections::{BTreeMap, BTreeSet};

use ignite_core::budget::{Budget, ClassRates, billions_text};
use ignite_core::config::Gate;
use ignite_core::gate::{self, GateCheck, GateInput, GatePlan, History, Planned, Why, cost_of};
use ignite_core::ids::{CheckName, TicketId};
use ignite_core::inputs::{self, CrateGraph};
use ignite_core::manifest::{Family, Kind};
use ignite_core::protocol::hub::{CachedResult, CheckState, CheckStats, CheckStatsReply};
use ignite_core::score::{self, AreaRecord, Cost, ProductCatch};

use super::{Checks, OpenTree, Prepared};
use crate::server::Context;

/// How many entries `ignite check --plan` lists under `top`.
const TOP: usize = 2;

/// The cost charged to a check that has never run, in a family no check of which has a cost.
const UNKNOWN_COST: Cost = Cost::Billions(1.0);

/// The start-up of a family whose start-up the hub has not estimated.
const NO_STARTUP: Cost = Cost::Billions(0.0);

/// What starting one of the family's commands costs, as the hub estimated it: in billions of
/// instructions where the family's machines count them, otherwise in CPU seconds.
fn family_startup(reply: &CheckStatsReply) -> Cost {
    match (reply.startup_billions, reply.startup_cpu_s) {
        (Some(billions), _) => Cost::Billions(billions),
        (None, Some(seconds)) => Cost::CpuSeconds(seconds),
        (None, None) => NO_STARTUP,
    }
}

/// The areas of a change: workspace packages and second-level directories, sorted.
pub(crate) fn areas(changed: &[String], graph: &CrateGraph) -> Vec<String> {
    let areas: BTreeSet<String> = changed
        .iter()
        .map(|path| inputs::area(path, graph).to_string())
        .collect();
    areas.into_iter().collect()
}

/// Product catches for one planning pass. Read before listing so a required check's family
/// can be brought into scope and listed even if it needs a cold build.
pub(super) struct GateCatches {
    pub areas: Vec<String>,
    pub records: Vec<ProductCatch>,
    pub recent: BTreeSet<CheckName>,
    pub regressions: BTreeMap<CheckName, TicketId>,
}

impl GateCatches {
    pub(super) fn read(
        checks: &Checks,
        ctx: &Context,
        tree: &OpenTree,
        changed: &[String],
    ) -> Self {
        let areas = areas(changed, &tree.graph);
        let now = ctx.clock.now();
        let records = product_catches(checks, ctx, tree);
        let regressions = super::verbs::open_regressions(ctx);
        let recent = records
            .iter()
            .filter(|catch| catch.applies(&areas, now) && !regressions.contains_key(&catch.check))
            .map(|catch| catch.check.clone())
            .collect();
        Self {
            areas,
            records,
            recent,
            regressions,
        }
    }
}

fn product_catches(checks: &Checks, ctx: &Context, tree: &OpenTree) -> Vec<ProductCatch> {
    checks.history.product_fixes().map_or_else(
        |error| {
            eprintln!("ignite: no product catch history from the hub: {error}");
            Vec::new()
        },
        |fixes| score::product_catches(fixes, &tree.graph, ctx.git.as_ref(), ctx.clock.now()),
    )
}

/// Reads check records and uses only product catches in their failure counts.
pub(super) fn scoring_stats<'a>(
    checks: &Checks,
    families: impl Iterator<Item = &'a str>,
    areas: &[String],
    catches: &[ProductCatch],
) -> BTreeMap<String, CheckStatsReply> {
    let mut stats = super::sources::stats_of(checks.history.as_ref(), families, areas);
    for reply in stats.values_mut() {
        score::use_product_catches(reply, catches, areas);
    }
    stats
}

/// Everything the gate is chosen from, for one tree.
pub(crate) struct GateView {
    pub areas: Vec<String>,
    pub checks: Vec<GateCheck>,
    /// The hub's record of each check, where it has one.
    pub stats: BTreeMap<CheckName, CheckStats>,
    /// Families whose scores use the classified outcome history.
    product_scored: BTreeSet<String>,
    /// The ticket being landed, whose own regression's check must run in this gate.
    pub landing: Option<TicketId>,
    pub recent_product_catches: BTreeSet<CheckName>,
    deleted_inputs: BTreeSet<CheckName>,
    /// Build products the tree has never built.
    pub unbuilt: BTreeSet<String>,
    /// Billions of instructions per second of wall time that the gate's check commands
    /// achieve, which converts `[gate] seconds` to instructions.
    pub rate: f64,
    /// The checks the last gate of the same change chose by score, which count against
    /// the budget when cached (see `ignite_core::gate`).
    pub paid: BTreeSet<CheckName>,
}

/// Each class's rate for today from the hub, or the defaults when the hub cannot answer.
pub(crate) fn class_rates(ctx: &Context) -> ClassRates {
    ctx.hub.rates().unwrap_or_else(|error| {
        eprintln!("ignite: no class rates from the hub, using the defaults: {error}");
        ClassRates::default()
    })
}

/// What the hub knows of `check`, a check of the regression family `declared`, for scoring, or
/// `None` when the hub has switched it off. `reply` is the family's record at the hub.
pub(crate) fn scored_check(
    check: &CheckName,
    declared: &Family,
    reply: Option<&CheckStatsReply>,
    regression: Option<TicketId>,
) -> Option<GateCheck> {
    let record = reply.and_then(|r| r.checks.iter().find(|s| s.check == *check));
    if record.is_some_and(|s| s.state == CheckState::Off) {
        return None;
    }
    Some(GateCheck {
        check: check.clone(),
        always: declared.always,
        edited: false,
        cached: false,
        history: record.and_then(history),
        family_rate: reply.map_or(0.0, |r| r.family_rate),
        family_median_cost: reply.map_or(UNKNOWN_COST, family_median_cost),
        family_startup: reply.map_or(NO_STARTUP, family_startup),
        needs: declared.needs.clone(),
        group: None,
        regression,
    })
}

impl GateView {
    /// The gate checks of the regression families in `prepared`, with `hits` in the same order
    /// as their checks. A check the hub has switched off is left out, and a cached result of an
    /// uncached check is ignored. A check in `regressions` has an open regression ticket, so the
    /// planner can judge whether the change needs it to run anyway.
    pub(super) fn new<'a>(
        prepared: &[Prepared],
        hits: &[Option<CachedResult>],
        family: impl Fn(&str) -> Option<&'a Family>,
        stats: &BTreeMap<String, CheckStatsReply>,
        changed: &[String],
        deleted_inputs: BTreeSet<CheckName>,
        catches: GateCatches,
    ) -> GateView {
        let mut view = GateView {
            areas: catches.areas,
            checks: Vec::new(),
            stats: BTreeMap::new(),
            product_scored: stats
                .values()
                .filter(|reply| reply.runs.is_some())
                .map(|reply| reply.family.clone())
                .collect(),
            landing: None,
            recent_product_catches: catches.recent,
            deleted_inputs,
            unbuilt: BTreeSet::new(),
            rate: ClassRates::default().land(),
            paid: BTreeSet::new(),
        };
        let mut hits = hits.iter();
        for prepared in prepared {
            let declared = family(&prepared.family);
            let reply = stats.get(&prepared.family);
            for keyed in &prepared.keyed {
                let hit = hits.next().and_then(Option::as_ref);
                let Some(declared) = declared.filter(|f| f.kind == Kind::Regression) else {
                    continue;
                };
                let check = &keyed.listed.check;
                let regression = catches.regressions.get(check).cloned();
                let Some(mut scored) = scored_check(check, declared, reply, regression) else {
                    continue;
                };
                let record = reply.and_then(|r| r.checks.iter().find(|s| s.check == *check));
                let check_state = record.map_or(CheckState::On, |s| s.state);
                scored.always = declared.always_for(changed);
                scored.edited = edited(check, changed);
                scored.cached = hit.is_some() && check_state == CheckState::On;
                view.checks.push(scored);
                if let Some(record) = record {
                    view.stats.insert(check.clone(), record.clone());
                }
            }
        }
        view
    }

    /// The gate's budget: `[gate] seconds` at [`GateView::rate`].
    pub(crate) fn budget(&self, gate: &Gate) -> Budget {
        Budget {
            threshold: gate.threshold,
            seconds: gate.seconds,
            rate: self.rate,
        }
    }

    pub(crate) fn plan(&self, gate: &Gate) -> GatePlan {
        gate::plan(
            &GateInput {
                areas: &self.areas,
                checks: &self.checks,
                unbuilt: &self.unbuilt,
                landing: self.landing.as_ref(),
                deleted_inputs: &self.deleted_inputs,
                recent_product_catches: &self.recent_product_catches,
                paid: &self.paid,
                billions_per_cpu_second: score::BILLIONS_PER_CPU_SECOND,
            },
            &self.budget(gate),
        )
    }

    /// Why a scored check is worth its cost: its worst record over the change's areas.
    fn reason(&self, planned: &Planned) -> String {
        match planned.why {
            Why::Always => "in the always set".to_owned(),
            Why::Edited => "edited by this change".to_owned(),
            Why::DeletedInput => "an input was deleted by this change".to_owned(),
            Why::Fixes => "fixes its regression".to_owned(),
            Why::RecentProductCatch => {
                "product bug in this area, fixed in the last 14 days".to_owned()
            }
            Why::Scored => {
                let Some(stats) = self.stats.get(&planned.check) else {
                    return "never run".to_owned();
                };
                let rate = self
                    .checks
                    .iter()
                    .find(|c| c.check == planned.check)
                    .map_or(0.0, |c| c.family_rate);
                let worst = stats
                    .areas
                    .iter()
                    .filter(|a| self.areas.contains(&a.area))
                    .max_by(|a, b| {
                        let p = |s: &&ignite_core::protocol::hub::AreaStats| {
                            score::probability_over_areas(
                                &[AreaRecord {
                                    runs: s.runs,
                                    failures: s.failures,
                                }],
                                rate,
                            )
                        };
                        p(a).total_cmp(&p(b))
                    });
                match worst {
                    Some(a) if self.product_scored.contains(planned.check.family()) => {
                        format!("{} product catches in {} runs", a.failures, a.runs)
                    }
                    Some(a) => format!("{} of {} runs failed", a.failures, a.runs),
                    None => format!("family rate {rate}"),
                }
            }
        }
    }
}

/// What tip verification scores a round's checks by: the areas the landings since the last
/// promoted tip touched, and the hub's record of each check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scoring {
    pub areas: Vec<String>,
    /// One per check the hub has not switched off, with its open regression ticket.
    pub checks: Vec<GateCheck>,
}

impl super::Checks {
    /// The [`Scoring`] of `checks`, named with their families, in `tree`, for a round whose
    /// landings changed `changed`.
    pub(crate) fn scoring(
        &self,
        ctx: &Context,
        tree: &ignite_core::ids::TreeId,
        changed: &[String],
        checks: &[(CheckName, String)],
    ) -> ignite_core::traits::Result<Scoring> {
        let (view, tree) = self.open(ctx, tree)?;
        let areas = areas(changed, &tree.graph);
        let families = checks.iter().map(|(_, f)| f.as_str());
        let catches = product_catches(self, ctx, &tree);
        let stats = scoring_stats(self, families, &areas, &catches);
        let regressions = super::verbs::open_regressions(ctx);
        let scored = checks
            .iter()
            .filter_map(|(check, family)| {
                let declared = &view.settings.manifest.families.get(family)?.family;
                let reply = stats.get(family.as_str());
                let mut scored =
                    scored_check(check, declared, reply, regressions.get(check).cloned())?;
                scored.always = declared.always_for(changed);
                Some(scored)
            })
            .collect();
        Ok(Scoring {
            areas,
            checks: scored,
        })
    }
}

/// The least chance of failing the gate credits a check with when ordering families, so a
/// check with no record of failing still counts for something.
const LEAST_CHANCE: f64 = 1e-4;

/// The cost of each family's checks to run in `gate_plan`, with the family's start-up, per
/// failure they are expected to find, in billions of instructions. The gate runs families in this order, smallest first, so a
/// cheap check likely to fail refuses a landing before a long family starts. A family with no
/// check to run (all from the cache) is absent and goes first.
pub(crate) fn run_order(view: &GateView, gate_plan: &GatePlan) -> BTreeMap<String, f64> {
    let mut totals: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for planned in &gate_plan.run {
        let Some(check) = view.checks.iter().find(|c| c.check == planned.check) else {
            continue;
        };
        let cost = check
            .history
            .as_ref()
            .map_or(check.family_median_cost, |h| h.median_cost)
            .billions(score::BILLIONS_PER_CPU_SECOND)
            .max(f64::EPSILON);
        // A score is a chance of failing per billion instructions; the always set has none.
        let chance = planned
            .score
            .map_or(check.family_rate, |score| score * cost)
            .max(LEAST_CHANCE);
        let total = totals
            .entry(planned.check.family().to_owned())
            .or_insert_with(|| {
                let startup = check
                    .family_startup
                    .billions(score::BILLIONS_PER_CPU_SECOND);
                (startup, 0.0)
            });
        total.0 += cost;
        total.1 += chance;
    }
    totals
        .into_iter()
        .map(|(family, (cost, chance))| (family, cost / chance))
        .collect()
}

/// A check's own file is one the change edits: its name, after the family, starts with that
/// path and goes on with `:` or `/` or ends there.
pub(super) fn edited(check: &CheckName, changed: &[String]) -> bool {
    let name = check.as_str().split_once(':').map_or("", |(_, name)| name);
    changed.iter().any(|path| {
        name.strip_prefix(path.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
    })
}

fn cost(stats: &CheckStats) -> Option<Cost> {
    stats.median_cost.map(|median| {
        if stats.unit == "cpu_s" {
            Cost::CpuSeconds(median)
        } else {
            Cost::Billions(median)
        }
    })
}

/// A check's history, or `None` for a check that has never run.
fn history(stats: &CheckStats) -> Option<History> {
    let median_cost = cost(stats)?;
    Some(History {
        areas: stats
            .areas
            .iter()
            .map(|a| {
                let record = AreaRecord {
                    runs: a.runs,
                    failures: a.failures,
                };
                (a.area.clone(), record)
            })
            .collect(),
        median_cost,
    })
}

/// The median of the family's checks' median costs, in billions of instructions.
fn family_median_cost(reply: &CheckStatsReply) -> Cost {
    let mut costs: Vec<f64> = reply
        .checks
        .iter()
        .filter_map(cost)
        .map(|c| c.billions(score::BILLIONS_PER_CPU_SECOND))
        .collect();
    if costs.is_empty() {
        return UNKNOWN_COST;
    }
    costs.sort_by(f64::total_cmp);
    Cost::Billions(costs[costs.len() / 2])
}

/// The labelled lines of `ignite check --plan`.
pub(crate) fn plan_lines(view: &GateView, plan: &GatePlan, gate: &Gate) -> Vec<(String, String)> {
    let chosen: Vec<&Planned> = plan.run.iter().chain(&plan.cached).collect();
    let always: Vec<&Planned> = chosen
        .iter()
        .copied()
        .filter(|p| p.why == Why::Always)
        .collect();
    let gated: Vec<&Planned> = chosen
        .iter()
        .copied()
        .filter(|p| p.why != Why::Always)
        .collect();
    let cached = plan.cached.iter().filter(|p| p.why != Why::Always).count();
    let mut recent: Vec<_> = gated
        .iter()
        .filter(|p| p.why == Why::RecentProductCatch)
        .collect();
    let mut lines = Vec::new();
    if !always.is_empty() {
        let names: Vec<&str> = always.iter().map(|p| short_name(&p.check)).collect();
        lines.push((
            "always".to_owned(),
            format!("{} checks ({})", always.len(), names.join(", ")),
        ));
    }
    lines.push(budget_line(&view.budget(gate)));
    let deleted = gated.iter().filter(|p| p.why == Why::DeletedInput).count();
    let selected = if recent.is_empty() && deleted == 0 {
        format!("above {}", gate.threshold)
    } else {
        "selected".to_owned()
    };
    lines.push((
        "gate".to_owned(),
        format!(
            "{} checks {selected}; {cached} cached, {} to run, about {} billion",
            gated.len(),
            gated.len() - cached,
            billions_text(plan.scored_cost + plan.forced_cost)
        ),
    ));
    if deleted > 0 {
        lines.push((
            "deleted".to_owned(),
            format!("{} forced by a deleted input", thousands(deleted)),
        ));
    }
    lines.extend(deferred_lines(view, plan));
    if !plan.known_failing.is_empty() {
        lines.push((
            "known".to_owned(),
            format!("{} checks have open regressions", plan.known_failing.len()),
        ));
    }
    recent.sort_by_key(|p| &p.check);
    lines.extend(recent.into_iter().map(|p| {
        (
            "recent".to_owned(),
            format!("{}  {}", p.check, view.reason(p)),
        )
    }));
    let top: Vec<_> = gated
        .into_iter()
        .filter(|p| p.why != Why::RecentProductCatch)
        .collect();
    lines.extend(top_lines(view, &top));
    lines
}

/// Checks the gate left to tip verification and the reason they were deferred.
fn deferred_lines(view: &GateView, plan: &GatePlan) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    if !plan.past_budget.is_empty() {
        lines.push((
            "past budget".to_owned(),
            format!(
                "{} checks above the threshold left to the tip ({} billion)",
                thousands(plan.past_budget.len()),
                billions_text(cost_of(&plan.past_budget))
            ),
        ));
    }
    if !plan.cold.is_empty() {
        lines.push((
            "cold".to_owned(),
            format!(
                "{} checks need a product this tree has never built, left to the tip ({} \
                 billion)",
                thousands(plan.cold.len()),
                billions_text(cost_of(&plan.cold))
            ),
        ));
    }
    let cached_names: BTreeSet<&CheckName> = view
        .checks
        .iter()
        .filter(|c| c.cached)
        .map(|c| &c.check)
        .collect();
    let affected = plan
        .tip
        .iter()
        .filter(|l| !cached_names.contains(&l.check))
        .count();
    lines.push((
        "tip".to_owned(),
        format!(
            "{} affected checks below the threshold",
            thousands(affected)
        ),
    ));
    lines
}

/// The `budget` line: the gate's budget in seconds, the rate it converts at, and the result.
pub(crate) fn budget_line(budget: &Budget) -> (String, String) {
    (
        "budget".to_owned(),
        format!(
            "{:.0} s of checks at {:.1} billion/s = {} billion",
            budget.seconds,
            budget.rate,
            billions_text(budget.billions())
        ),
    )
}

/// The best-scored chosen checks, a run of checks in one module with the same reason and score
/// shown as one `<module>::*` line.
fn top_lines(view: &GateView, gated: &[&Planned]) -> Vec<(String, String)> {
    let mut sorted: Vec<&Planned> = gated.to_vec();
    sorted.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .total_cmp(&a.score.unwrap_or(0.0))
            .then_with(|| a.check.cmp(&b.check))
    });
    let mut entries: Vec<(String, f64, String, usize)> = Vec::new();
    for planned in sorted {
        let score = planned.score.unwrap_or(0.0);
        let reason = view.reason(planned);
        let name = planned.check.as_str();
        if let Some(last) = entries.last_mut()
            && last.2 == reason
            && format!("{:.2}", last.1) == format!("{score:.2}")
            && module(&last.0) == module(name)
        {
            last.3 += 1;
            continue;
        }
        entries.push((name.to_owned(), score, reason, 1));
    }
    let entries: Vec<(String, f64, String)> = entries
        .into_iter()
        .take(TOP)
        .map(|(name, score, reason, count)| {
            let name = if count > 1 {
                format!("{}*", module(&name))
            } else {
                name
            };
            (name, score, reason)
        })
        .collect();
    let width = entries.iter().map(|e| e.0.len()).max().unwrap_or(0) + 6;
    entries
        .into_iter()
        .enumerate()
        .map(|(index, (name, score, reason))| {
            let label = if index == 0 { "top" } else { "" };
            (
                label.to_owned(),
                format!("{name:<width$}{score:.2}  {reason}"),
            )
        })
        .collect()
}

/// A check name up to and including its last `::` or `/`.
fn module(name: &str) -> &str {
    let end = match (name.rfind("::"), name.rfind('/')) {
        (Some(colons), Some(slash)) if slash > colons => slash + 1,
        (Some(colons), _) => colons + 2,
        (None, Some(slash)) => slash + 1,
        (None, None) => name.len(),
    };
    &name[..end]
}

/// A check's name without its family, as the always line shows it.
pub(crate) fn short_name(check: &CheckName) -> &str {
    check
        .as_str()
        .split_once(':')
        .map_or(check.as_str(), |(_, name)| name)
}

/// `1204` as `1,204`.
pub(crate) fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_modules() {
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(61), "61");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(module("rust:a::tests::x"), "rust:a::tests::");
        assert_eq!(module("shots:mode/phone"), "shots:mode/");
    }

    #[test]
    fn a_check_is_edited_when_its_own_file_is() {
        let check = CheckName::new("py:tools/tests/test_a.py::one").unwrap();
        assert!(edited(&check, &["tools/tests/test_a.py".to_owned()]));
        assert!(!edited(&check, &["tools/tests/test_a".to_owned()]));
        assert!(!edited(&check, &["tools/x.py".to_owned()]));
    }
}
