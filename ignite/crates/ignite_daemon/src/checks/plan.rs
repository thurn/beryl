//! From listed checks and the hub's record of them to the inputs of [`gate::plan`], and from a
//! [`GatePlan`] to the text of `ignite check --plan` (see "The threshold and the cap" in
//! `docs/ignite.md`).

use std::collections::{BTreeMap, BTreeSet};

use ignite_core::config::Gate;
use ignite_core::gate::{self, GateCheck, GateInput, GatePlan, History, Planned, Why};
use ignite_core::ids::CheckName;
use ignite_core::inputs::{self, CrateGraph};
use ignite_core::manifest::{Family, Kind};
use ignite_core::protocol::hub::{CachedResult, CheckState, CheckStats, CheckStatsReply};
use ignite_core::score::{self, AreaRecord, Cost};

use super::Prepared;

/// How many entries `ignite check --plan` lists under `top`.
const TOP: usize = 2;

/// The cost charged to a check that has never run, in a family no check of which has a cost.
const UNKNOWN_COST: Cost = Cost::Billions(1.0);

/// The areas of a change: workspace packages and second-level directories, sorted.
pub(crate) fn areas(changed: &[String], graph: &CrateGraph) -> Vec<String> {
    let areas: BTreeSet<String> = changed
        .iter()
        .map(|path| inputs::area(path, graph).to_string())
        .collect();
    areas.into_iter().collect()
}

/// Everything the gate is chosen from, for one tree.
pub(crate) struct GateView {
    pub areas: Vec<String>,
    pub checks: Vec<GateCheck>,
    /// The hub's record of each check, where it has one.
    pub stats: BTreeMap<CheckName, CheckStats>,
}

impl GateView {
    /// The gate checks of the regression families in `prepared`, with `hits` in the same order
    /// as their checks. A check the hub has switched off is left out, and a cached result of an
    /// uncached check is ignored.
    pub(crate) fn new<'a>(
        prepared: &[Prepared],
        hits: &[Option<CachedResult>],
        family: impl Fn(&str) -> Option<&'a Family>,
        stats: &BTreeMap<String, CheckStatsReply>,
        changed: &[String],
        areas: Vec<String>,
    ) -> GateView {
        let mut view = GateView {
            areas,
            checks: Vec::new(),
            stats: BTreeMap::new(),
        };
        let mut hits = hits.iter();
        for prepared in prepared {
            let declared = family(&prepared.family);
            let reply = stats.get(&prepared.family);
            let family_median = reply.map_or(UNKNOWN_COST, family_median_cost);
            for keyed in &prepared.keyed {
                let hit = hits.next().and_then(Option::as_ref);
                let Some(declared) = declared.filter(|f| f.kind == Kind::Regression) else {
                    continue;
                };
                let check = &keyed.listed.check;
                let record = reply.and_then(|r| r.checks.iter().find(|s| s.check == *check));
                let check_state = record.map_or(CheckState::On, |s| s.state);
                if check_state == CheckState::Off {
                    continue;
                }
                view.checks.push(GateCheck {
                    check: check.clone(),
                    always: declared.always,
                    edited: edited(check, changed),
                    cached: hit.is_some() && check_state == CheckState::On,
                    history: record.and_then(history),
                    family_rate: reply.map_or(0.0, |r| r.family_rate),
                    family_median_cost: family_median,
                    needs: declared.needs.clone(),
                    group: None,
                    regression: None,
                });
                if let Some(record) = record {
                    view.stats.insert(check.clone(), record.clone());
                }
            }
        }
        view
    }

    pub(crate) fn plan(&self, gate: &Gate) -> GatePlan {
        let missing = BTreeMap::new();
        gate::plan(
            &GateInput {
                areas: &self.areas,
                checks: &self.checks,
                missing_builds: &missing,
                landing: None,
                billions_per_cpu_second: score::BILLIONS_PER_CPU_SECOND,
            },
            gate,
        )
    }

    /// Why a scored check is worth its cost: its worst record over the change's areas.
    fn reason(&self, planned: &Planned) -> String {
        match planned.why {
            Why::Always => "in the always set".to_owned(),
            Why::Edited => "edited by this change".to_owned(),
            Why::Fixes => "fixes its regression".to_owned(),
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
                    Some(a) => format!("{} of {} runs failed", a.failures, a.runs),
                    None => format!("family rate {rate}"),
                }
            }
        }
    }
}

/// A check's own file is one the change edits: its name, after the family, starts with that
/// path and goes on with `:` or `/` or ends there.
fn edited(check: &CheckName, changed: &[String]) -> bool {
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
    let mut lines = Vec::new();
    if !always.is_empty() {
        let names: Vec<&str> = always.iter().map(|p| short_name(&p.check)).collect();
        lines.push((
            "always".to_owned(),
            format!("{} checks ({})", always.len(), names.join(", ")),
        ));
    }
    lines.push((
        "gate".to_owned(),
        format!(
            "{} checks above {}; {cached} cached, {} to run, about {:.0} billion",
            gated.len(),
            gate.threshold,
            gated.len() - cached,
            plan.scored_cost + plan.forced_cost
        ),
    ));
    if !plan.past_cap.is_empty() {
        lines.push((
            "past cap".to_owned(),
            format!(
                "{} checks above the threshold left to the tip",
                thousands(plan.past_cap.len())
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
    if !plan.known_failing.is_empty() {
        lines.push((
            "known".to_owned(),
            format!("{} checks have open regressions", plan.known_failing.len()),
        ));
    }
    lines.extend(top_lines(view, &gated));
    lines
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
