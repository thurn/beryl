//! Gate selection: which checks run before a landing and which are left to tip verification.
//!
//! [`plan`] is a pure function from what a change touches and what the hub knows of each check
//! it affects to a [`GatePlan`]. See "Where regression checks run" in `docs/ignite.md`.
//!
//! - The always set (checks in a family marked `always = true`) is looked up or run on every
//!   landing, outside the threshold and the cap.
//! - A check whose own file the change edits always runs, also outside the cap.
//! - A check with an open regression ticket is left out and reported as known to fail, unless
//!   the ticket being landed is that regression's, in which case it must run, outside the cap.
//! - Every other check is chosen greedily by [`crate::score`]: the highest score first, until the
//!   best score left is below the threshold or the next check would take the spending past the
//!   cap. A build the tree lacks is charged to the first chosen check that needs it, and the
//!   checks after it are scored again without that build.
//! - Checks in one group (checks that failed in exactly the same runs) are scored as one check
//!   with their summed cost, and are chosen or left together.
//!   A member that must run anyway (always, edited or being fixed) leaves its group first.
//! - A chosen check whose result is cached is answered from the cache and costs nothing against
//!   the cap.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::Gate;
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
    /// Build products the tree does not hold, with each one's estimated cost in billions of
    /// instructions. A product not listed is current.
    pub missing_builds: &'a BTreeMap<String, f64>,
    /// The ticket being landed.
    pub landing: Option<&'a TicketId>,
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
    /// Its score is at or above the threshold and it fit under the cap.
    Scored,
}

/// A check the gate runs or answers from the cache.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    pub check: CheckName,
    pub why: Why,
    /// The score when it was chosen; `None` for the always set.
    pub score: Option<f64>,
    /// Missing build products charged to this check, which must be built before it runs.
    pub builds: Vec<String>,
}

/// An affected check the gate does not run, with its score when the selection stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub check: CheckName,
    pub score: f64,
}

/// Where each affected check is answered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GatePlan {
    /// Checks to run, in order: the always set, then the checks that must run, then the scored
    /// checks in the order they were chosen.
    pub run: Vec<Planned>,
    /// Chosen checks whose outcome comes from the result cache, in the same order.
    pub cached: Vec<Planned>,
    /// Checks at or above the threshold that the cap left to tip verification, by score.
    pub past_cap: Vec<Left>,
    /// Checks below the threshold, left to tip verification, by score.
    pub tip: Vec<Left>,
    /// Checks left out because a regression ticket for them is open, with that ticket.
    pub known_failing: Vec<(CheckName, TicketId)>,
    /// Billions of instructions the scored checks spend, builds included: what counts against
    /// the cap.
    pub scored_cost: f64,
    /// Billions of instructions the always set and the other checks that must run spend, builds
    /// included.
    pub forced_cost: f64,
}

/// One check, or one group of checks, as the greedy selection sees it.
struct Unit<'a> {
    /// Sorted by name.
    members: Vec<&'a GateCheck>,
    probability: f64,
    /// The members' summed cost, cached members included: the score's denominator before builds.
    cost: f64,
    /// The summed cost of the members that would run.
    run_cost: f64,
    /// The builds the members that would run need.
    needs: BTreeSet<&'a str>,
}

impl Unit<'_> {
    fn unpaid(&self, missing: &BTreeMap<String, f64>, paid: &BTreeSet<&str>) -> f64 {
        self.needs
            .iter()
            .filter(|need| !paid.contains(*need))
            .filter_map(|need| missing.get(*need))
            .sum()
    }

    fn score(&self, missing: &BTreeMap<String, f64>, paid: &BTreeSet<&str>) -> f64 {
        per_cost(self.probability, self.cost + self.unpaid(missing, paid))
    }

    fn name(&self) -> &CheckName {
        &self.members[0].check
    }
}

/// Decides where each affected check runs. The same input gives the same plan, whatever the
/// order of `input.checks`.
pub fn plan(input: &GateInput<'_>, gate: &Gate) -> GatePlan {
    let mut checks: Vec<&GateCheck> = input.checks.iter().collect();
    checks.sort_by(|a, b| a.check.cmp(&b.check));

    let mut plan = GatePlan::default();
    let mut paid: BTreeSet<&str> = BTreeSet::new();
    let mut scored: Vec<&GateCheck> = Vec::new();
    let mut must_run: Vec<(&GateCheck, Why)> = Vec::new();
    let mut always: Vec<&GateCheck> = Vec::new();
    for check in checks {
        if check.always {
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

    let forced = always
        .into_iter()
        .map(|check| (check, Why::Always))
        .chain(must_run);
    for (check, why) in forced {
        let score = (why != Why::Always).then(|| {
            let unit = unit(input, vec![check]);
            unit.score(input.missing_builds, &paid)
        });
        let mut planned = Planned {
            check: check.check.clone(),
            why,
            score,
            builds: Vec::new(),
        };
        if check.cached {
            plan.cached.push(planned);
            continue;
        }
        let needs = check.needs.iter().map(String::as_str).collect();
        plan.forced_cost += cost(input, check) + charge(input, &needs, &mut paid, &mut planned);
        plan.run.push(planned);
    }

    choose(input, gate, units(input, scored), &mut paid, &mut plan);
    plan
}

/// Chooses among `units` greedily by score until the best score left is below the threshold or
/// the next unit would take the spending past the cap, then leaves the rest to the tip.
fn choose<'a>(
    input: &GateInput<'a>,
    gate: &Gate,
    mut units: Vec<Unit<'a>>,
    paid: &mut BTreeSet<&'a str>,
    plan: &mut GatePlan,
) {
    loop {
        let best = units
            .iter()
            .enumerate()
            .map(|(index, unit)| (index, unit.score(input.missing_builds, paid)))
            .max_by(|(a, a_score), (b, b_score)| {
                a_score
                    .total_cmp(b_score)
                    .then_with(|| units[*b].name().cmp(units[*a].name()))
            });
        let Some((index, score)) = best else { break };
        if score < gate.threshold {
            break;
        }
        let unit = &units[index];
        let spend = unit.run_cost + unit.unpaid(input.missing_builds, paid);
        if plan.scored_cost + spend > gate.cap {
            break;
        }
        let unit = units.swap_remove(index);
        plan.scored_cost += spend;
        let mut charged = false;
        for member in unit.members {
            let mut planned = Planned {
                check: member.check.clone(),
                why: Why::Scored,
                score: Some(score),
                builds: Vec::new(),
            };
            if member.cached {
                plan.cached.push(planned);
            } else {
                if !charged {
                    charge(input, &unit.needs, paid, &mut planned);
                    charged = true;
                }
                plan.run.push(planned);
            }
        }
    }

    let mut left: Vec<(f64, &Unit<'_>)> = units
        .iter()
        .map(|unit| (unit.score(input.missing_builds, paid), unit))
        .collect();
    left.sort_by(|(a_score, a), (b_score, b)| {
        b_score
            .total_cmp(a_score)
            .then_with(|| a.name().cmp(b.name()))
    });
    for (score, unit) in left {
        let target = if score >= gate.threshold {
            &mut plan.past_cap
        } else {
            &mut plan.tip
        };
        target.extend(unit.members.iter().map(|member| Left {
            check: member.check.clone(),
            score,
        }));
    }
}

/// Marks each missing build in `needs` that nothing has paid for as paid by `planned`, and
/// returns what those builds cost.
fn charge<'a>(
    input: &GateInput<'a>,
    needs: &BTreeSet<&str>,
    paid: &mut BTreeSet<&'a str>,
    planned: &mut Planned,
) -> f64 {
    let mut spent = 0.0;
    for (product, estimate) in input.missing_builds {
        if needs.contains(product.as_str()) && paid.insert(product.as_str()) {
            planned.builds.push(product.clone());
            spent += estimate;
        }
    }
    spent
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
