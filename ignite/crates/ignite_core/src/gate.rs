//! Gate selection: which checks run before a landing and which are left to tip verification.
//!
//! [`plan`] is a pure function from what a change touches and what the hub knows of each check
//! it affects to a [`GatePlan`]. See "Where regression checks run" in `docs/ignite.md`.
//!
//! - The always set (checks in a family marked `always = true`) is looked up or run on every
//!   landing, outside the threshold and the budget.
//! - A check whose own file the change edits always runs, also outside the budget.
//! - A check with an open regression ticket is left out and reported as known to fail, unless
//!   the ticket being landed is that regression's, in which case it must run, outside the
//!   budget. This holds in the always set too, unless the change edits the check's own file.
//!   The landing passes a check it found failing at the tip it was rebased onto here as one
//!   with an open regression ticket.
//! - A check that needs a build product the tree has never built is left to tip verification
//!   unless it must run anyway: the gate runs checks on what the tree has already compiled, and
//!   a cold build belongs to the tip. An incremental rebuild of a product the tree has built
//!   before is simply done. Builds are outside the budget either way. A product a check that
//!   must run needs is built for it, so it is not cold for the scored checks.
//! - Every other check is chosen greedily by [`crate::score`]: the highest score first, until the
//!   best score left is below the threshold or the next check would take the spending past the
//!   budget ([`Budget`]).
//! - Checks in one group (checks that failed in exactly the same runs) are scored as one check
//!   with their summed cost, and are chosen or left together.
//!   A member that must run anyway (always, edited or being fixed) leaves its group first.
//! - A chosen check whose result is cached is answered from the cache and costs nothing against
//!   the budget, unless an earlier passing gate of the same change chose it by score
//!   ([`GateInput::paid`]). That gate already spent the change's budget on it, so the check
//!   counts against this gate's budget as well, with its family's start-up, and a second gate
//!   of an unchanged change (`ignite check` and then `ignite land`) chooses the same checks
//!   instead of spending a second budget on checks with lower scores.
//! - A check's cost leaves out the start-up of the command it runs in, which its family pays
//!   once ([`GateCheck::family_startup`]): with the first check of the family that must run, or
//!   else with the first one chosen by score, when it counts against the budget. A score
//!   divides by the check's own cost alone, so a family's first check is not passed over for the
//!   start-up the checks after it share.
//!
//! Tip verification splits a round's checks with the same function, with a threshold of zero
//! and its own budget.

use std::collections::{BTreeMap, BTreeSet};

use crate::budget::Budget;
use crate::ids::{CheckName, TicketId};
use crate::score::{AreaRecord, Cost, per_cost, probability_over_areas};

/// What the hub knows of one check the change affects.
#[derive(Clone, Debug, PartialEq)]
pub struct GateCheck {
    pub check: CheckName,
    /// In a family marked `always = true`.
    pub always: bool,
    /// The change edits the check's own file.
    pub edited: bool,
    /// The result cache holds this check's outcome for the tree's input hash.
    pub cached: bool,
    /// The check's runs, or `None` for a check that has never run.
    pub history: Option<History>,
    /// The family's share of runs that failed.
    pub family_rate: f64,
    /// The family's median cost, which a check that has never run is charged.
    pub family_median_cost: Cost,
    /// What starting one of the family's commands costs, paid once for the family however many
    /// of its checks run; zero while unknown.
    pub family_startup: Cost,
    /// Build products that must be current before the check runs.
    pub needs: Vec<String>,
    /// Checks that failed in exactly the same runs share a group.
    pub group: Option<String>,
    /// The open regression ticket for this check, if one is open.
    pub regression: Option<TicketId>,
}

/// One check's record.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    /// Runs and failures after changes to each area.
    pub areas: BTreeMap<String, AreaRecord>,
    /// The median cost of its last 20 runs.
    pub median_cost: Cost,
}

/// Everything [`plan`] reads.
#[derive(Clone, Copy, Debug)]
pub struct GateInput<'a> {
    /// The areas the change touches: workspace packages, and second-level directories outside the
    /// workspaces.
    pub areas: &'a [String],
    pub checks: &'a [GateCheck],
    /// Build products the tree has never built. A check that needs one is left to tip
    /// verification unless it must run.
    pub unbuilt: &'a BTreeSet<String>,
    /// The ticket being landed.
    pub landing: Option<&'a TicketId>,
    /// The checks an earlier passing gate of the same change chose by score. The budget belongs
    /// to the change, not to each gate: such a check, when cached, still counts against it.
    pub paid: &'a BTreeSet<CheckName>,
    /// The rate at which a cost known only in CPU seconds converts to billions of instructions
    /// ([`crate::score::BILLIONS_PER_CPU_SECOND`]).
    pub billions_per_cpu_second: f64,
}

/// Why a check is in the gate.
#[derive(Clone, Debug, PartialEq)]
pub enum Why {
    /// In the always set.
    Always,
    /// The change edits it.
    Edited,
    /// The ticket being landed is this check's regression ticket.
    Fixes,
    /// Its score is at or above the threshold and it fit within the budget.
    Scored,
}

/// A check the gate runs or answers from the cache.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    pub check: CheckName,
    pub why: Why,
    /// The score when it was chosen; `None` for the always set.
    pub score: Option<f64>,
    /// What one run of it costs, in billions of instructions; for a cached check, what the
    /// cache saved.
    pub cost: f64,
}

/// An affected check the gate does not run, with its score when the selection stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub check: CheckName,
    pub score: f64,
    /// What one run of it costs, in billions of instructions.
    pub cost: f64,
}

/// Where each affected check is answered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GatePlan {
    /// Checks to run, in order: the always set, then the checks that must run, then the scored
    /// checks in the order they were chosen.
    pub run: Vec<Planned>,
    /// Chosen checks whose outcome comes from the result cache, in the same order.
    pub cached: Vec<Planned>,
    /// Checks at or above the threshold that the budget left to tip verification, by score.
    pub past_budget: Vec<Left>,
    /// Checks at or above the threshold that need a product the tree has never built, left to
    /// tip verification, by score.
    pub cold: Vec<Left>,
    /// Checks below the threshold, left to tip verification, by score.
    pub tip: Vec<Left>,
    /// Checks left out because a regression ticket for them is open, with that ticket.
    pub known_failing: Vec<(CheckName, TicketId)>,
    /// Billions of instructions the scored checks spend, with the start-ups of the families
    /// they start: what counts against the budget.
    pub scored_cost: f64,
    /// Billions of instructions the always set and the other checks that must run spend, with
    /// the start-ups of their families.
    pub forced_cost: f64,
    /// Billions of the budget that an earlier passing gate of the same change spent on checks
    /// chosen by score that this gate answers from the cache, with their families' start-ups
    /// ([`GateInput::paid`]).
    pub paid_cost: f64,
}

/// The summed cost of `left`, in billions of instructions.
pub fn cost_of(left: &[Left]) -> f64 {
    left.iter().map(|left| left.cost).sum()
}

/// One check, or one group of checks, as the greedy selection sees it.
struct Unit<'a> {
    /// Sorted by name.
    members: Vec<&'a GateCheck>,
    probability: f64,
    /// The members' summed cost, cached members included: the score's denominator.
    cost: f64,
    /// The summed cost of the members that would run.
    run_cost: f64,
    /// The builds the members that would run need.
    needs: BTreeSet<&'a str>,
}

impl Unit<'_> {
    fn score(&self) -> f64 {
        per_cost(self.probability, self.cost)
    }

    fn name(&self) -> &CheckName {
        &self.members[0].check
    }

    /// A member that would run needs a product the tree has never built.
    fn cold(&self, unbuilt: &BTreeSet<String>) -> bool {
        self.needs.iter().any(|need| unbuilt.contains(*need))
    }
}

/// Decides where each affected check runs. The same input gives the same plan, whatever the
/// order of `input.checks`.
pub fn plan(input: &GateInput<'_>, budget: &Budget) -> GatePlan {
    let mut checks: Vec<&GateCheck> = input.checks.iter().collect();
    checks.sort_by(|a, b| a.check.cmp(&b.check));

    let mut plan = GatePlan::default();
    let mut scored: Vec<&GateCheck> = Vec::new();
    let mut must_run: Vec<(&GateCheck, Why)> = Vec::new();
    let mut always: Vec<&GateCheck> = Vec::new();
    for check in checks {
        // A check that already fails at the tip is known to fail even in the always set: the
        // set exists so that no landing produces a tip that fails it, and this tip already does.
        let known = check
            .regression
            .as_ref()
            .is_some_and(|ticket| input.landing != Some(ticket));
        if check.always && !known {
            always.push(check);
        } else if check.edited {
            must_run.push((check, Why::Edited));
        } else if let Some(ticket) = &check.regression {
            if input.landing == Some(ticket) {
                must_run.push((check, Why::Fixes));
            } else {
                plan.known_failing
                    .push((check.check.clone(), ticket.clone()));
            }
        } else {
            scored.push(check);
        }
    }

    let forced: Vec<(&GateCheck, Why)> = always
        .into_iter()
        .map(|check| (check, Why::Always))
        .chain(must_run)
        .collect();
    // A product that a check which must run needs is built for it, so it is no longer cold for
    // the checks chosen by score.
    let mut unbuilt = input.unbuilt.clone();
    for (check, _) in forced.iter().filter(|(check, _)| !check.cached) {
        for need in &check.needs {
            unbuilt.remove(need);
        }
    }
    // The families whose start-up a check that runs has paid.
    let mut started: BTreeSet<&str> = BTreeSet::new();
    for (check, why) in forced {
        let score = (why != Why::Always).then(|| unit(input, vec![check]).score());
        let planned = Planned {
            check: check.check.clone(),
            why,
            score,
            cost: cost(input, check),
        };
        if check.cached {
            plan.cached.push(planned);
            continue;
        }
        plan.forced_cost += planned.cost + startups(input, [check], &mut started);
        plan.run.push(planned);
    }

    choose(
        input,
        budget,
        &unbuilt,
        units(input, scored),
        &mut started,
        &mut plan,
    );
    plan
}

/// The start-ups of the families of `checks` that are not in `started` yet, in billions of
/// instructions, adding those families to it.
fn startups<'a>(
    input: &GateInput<'_>,
    checks: impl IntoIterator<Item = &'a GateCheck>,
    started: &mut BTreeSet<&'a str>,
) -> f64 {
    checks
        .into_iter()
        .filter(|check| started.insert(check.check.family()))
        .map(|check| check.family_startup.billions(input.billions_per_cpu_second))
        .sum()
}

/// What `startups` would add for the members of `unit` that run, without paying them.
fn unpaid(input: &GateInput<'_>, unit: &Unit<'_>, started: &BTreeSet<&str>) -> f64 {
    let mut seen = started.clone();
    startups(
        input,
        unit.members.iter().copied().filter(|m| !m.cached),
        &mut seen,
    )
}

/// Chooses among `units` greedily by score until the best score left is below the threshold or
/// the next unit would take the spending past the budget, then leaves the rest to the tip.
fn choose<'a>(
    input: &GateInput<'a>,
    budget: &Budget,
    unbuilt: &BTreeSet<String>,
    units: Vec<Unit<'a>>,
    started: &mut BTreeSet<&'a str>,
    plan: &mut GatePlan,
) {
    let limit = budget.billions();
    let (cold, mut units): (Vec<Unit<'a>>, Vec<Unit<'a>>) =
        units.into_iter().partition(|unit| unit.cold(unbuilt));
    // Once the budget would be passed, only units that cost nothing remain eligible: a unit whose
    // members are all cached spends no instructions, so the budget has no reason to leave it out.
    // A unit that would start a family costs its start-up.
    let mut spent_out = false;
    // The families whose start-up an earlier gate of the change paid for a check it chose.
    let mut paid_started: BTreeSet<&str> = BTreeSet::new();
    loop {
        let best = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| !spent_out || unit.run_cost + unpaid(input, unit, started) <= 0.0)
            .map(|(index, unit)| (index, unit.score()))
            .max_by(|(a, a_score), (b, b_score)| {
                a_score
                    .total_cmp(b_score)
                    .then_with(|| units[*b].name().cmp(units[*a].name()))
            });
        let Some((index, score)) = best else { break };
        if score < budget.threshold {
            break;
        }
        let startup = unpaid(input, &units[index], started);
        if plan.scored_cost + plan.paid_cost + units[index].run_cost + startup > limit {
            spent_out = true;
            continue;
        }
        let unit = units.swap_remove(index);
        let running = unit.members.iter().copied().filter(|m| !m.cached);
        plan.scored_cost += unit.run_cost + startups(input, running, started);
        plan.paid_cost += paid_charge(input, &unit, started, &mut paid_started);
        for member in unit.members {
            let planned = Planned {
                check: member.check.clone(),
                why: Why::Scored,
                score: Some(score),
                cost: cost(input, member),
            };
            if member.cached {
                plan.cached.push(planned);
            } else {
                plan.run.push(planned);
            }
        }
    }

    let mut left: Vec<(bool, f64, &Unit<'_>)> = units
        .iter()
        .map(|unit| (false, unit.score(), unit))
        .chain(cold.iter().map(|unit| (true, unit.score(), unit)))
        .collect();
    left.sort_by(|(_, a_score, a), (_, b_score, b)| {
        b_score
            .total_cmp(a_score)
            .then_with(|| a.name().cmp(b.name()))
    });
    for (cold, score, unit) in left {
        let target = if score < budget.threshold {
            &mut plan.tip
        } else if cold {
            &mut plan.cold
        } else {
            &mut plan.past_budget
        };
        target.extend(unit.members.iter().map(|member| Left {
            check: member.check.clone(),
            score,
            cost: cost(input, member),
        }));
    }
}

/// What the cached members of `unit` that an earlier gate of the change chose by score charge
/// the budget: their costs, and the start-up of each of their families that neither a check
/// this gate runs nor an earlier such charge has paid, adding those families to `paid_started`.
fn paid_charge<'a>(
    input: &GateInput<'a>,
    unit: &Unit<'a>,
    started: &BTreeSet<&'a str>,
    paid_started: &mut BTreeSet<&'a str>,
) -> f64 {
    unit.members
        .iter()
        .copied()
        .filter(|member| member.cached && input.paid.contains(&member.check))
        .map(|member| {
            let family = member.check.family();
            let startup = if started.contains(family) || !paid_started.insert(family) {
                0.0
            } else {
                member
                    .family_startup
                    .billions(input.billions_per_cpu_second)
            };
            cost(input, member) + startup
        })
        .sum()
}

/// The cost of one run of `check`, in billions of instructions.
fn cost(input: &GateInput<'_>, check: &GateCheck) -> f64 {
    let cost = check
        .history
        .as_ref()
        .map_or(check.family_median_cost, |history| history.median_cost);
    cost.billions(input.billions_per_cpu_second)
}

/// The chance that `check` fails after the change: certain for a check that has never run.
fn probability(input: &GateInput<'_>, check: &GateCheck) -> f64 {
    let Some(history) = &check.history else {
        return 1.0;
    };
    let areas: Vec<AreaRecord> = input
        .areas
        .iter()
        .map(|area| history.areas.get(area).copied().unwrap_or_default())
        .collect();
    probability_over_areas(&areas, check.family_rate)
}

fn unit<'a>(input: &GateInput<'a>, members: Vec<&'a GateCheck>) -> Unit<'a> {
    let probability = members
        .iter()
        .map(|check| probability(input, check))
        .fold(0.0, f64::max);
    let total: f64 = members.iter().map(|check| cost(input, check)).sum();
    let running = || members.iter().filter(|check| !check.cached);
    let run_cost = running().map(|check| cost(input, check)).sum();
    let needs = running()
        .flat_map(|check| check.needs.iter().map(String::as_str))
        .collect();
    Unit {
        probability,
        cost: total,
        run_cost,
        needs,
        members,
    }
}

/// Groups `checks`, which are sorted by name, into units: one per group, one per check without.
fn units<'a>(input: &GateInput<'a>, checks: Vec<&'a GateCheck>) -> Vec<Unit<'a>> {
    let mut groups: BTreeMap<&str, Vec<&GateCheck>> = BTreeMap::new();
    let mut units = Vec::new();
    for check in checks {
        match &check.group {
            Some(group) => groups.entry(group.as_str()).or_default().push(check),
            None => units.push(unit(input, vec![check])),
        }
    }
    units.extend(groups.into_values().map(|members| unit(input, members)));
    units
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
