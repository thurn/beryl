//! `ignite gate replay`: the gate's settings worked out from history. See "The threshold and the
//! cap" in `docs/ignite.md`.
//!
//! - A **catch** is a failure of a regression check that a later change made pass again, from
//!   the old run log and its verdicts ([`ignite_store::import::runlog::catches`]). Exploration
//!   lanes and families, families in the always set and checks that are switched off are left
//!   out.
//! - Each catch is scored as the gate would have scored its check for the change that failed,
//!   from the check records the hub holds now, less the catch itself: the hub's counts include
//!   the failure being replayed, which would make every catch look likelier than the gate could
//!   have known. The run and the failure of the catch are taken out of its check's counts for
//!   the catch's areas (leave-one-out). Other checks' counts keep their own failures of the
//!   same change, which the log does not link to this one. The threshold is the largest of those scores at
//!   which the gate still keeps [`KEEP`] of the catches: a catch is kept when its check's score
//!   is at or above the threshold.
//! - The cap is the [`KEEP`] quantile (nearest rank) of what the gate would have spent on the
//!   checks chosen by score, at that threshold and with no cap, for the changes that were caught.
//!   Only the checks the change could affect are costed: the catch's own check and the checks
//!   with a record after changes to one of the catch's areas. The others' inputs were not
//!   touched, so on a real landing the result cache answers them for nothing.
//! - The replay knows neither which builds each tree held nor what the result cache held beyond
//!   that, so it charges no builds and counts every affected chosen check as run.
//!
//! It prints the two values, the share of catches each nearby threshold keeps, and the `[gate]`
//! lines for `ignite.toml`. It writes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ignite_core::admission::median;
use ignite_core::config::{Config, Gate};
use ignite_core::gate::{GateCheck, GateInput, GatePlan, History, Why, plan};
use ignite_core::manifest::Kind;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::{CheckState, CheckStatsReply};
use ignite_core::score::{AreaRecord, BILLIONS_PER_CPU_SECOND, Cost};
use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;
use ignite_git::GitCli;
use ignite_store::import::RunLogSource;
use ignite_store::import::runlog::{self, Catch};

use crate::args::GateCommand;
use crate::manifest::{CONFIG_FILE, crate_graph, read_manifest};

/// The share of catches the threshold keeps, and the quantile of gate costs the cap covers.
pub const KEEP: f64 = 0.95;
/// How many candidate thresholds on each side of the chosen one the output lists.
const NEARBY: usize = 2;

/// One candidate threshold.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub threshold: f64,
    /// Catches whose check scored at or above the threshold.
    pub kept: usize,
    /// The [`KEEP`] quantile of the gates' scored cost at this threshold, in billions.
    pub cap: f64,
}

/// What the replay found.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    /// Catches that were scored.
    pub catches: usize,
    /// Catches left out because the hub has no scored check of that name.
    pub unscored: usize,
    /// Candidates around the chosen one, from the largest threshold down.
    pub nearby: Vec<Row>,
    /// The chosen threshold's index in `nearby`.
    pub chosen: usize,
}

impl Replay {
    pub fn chosen(&self) -> &Row {
        &self.nearby[self.chosen]
    }

    /// The values as they go in `ignite.toml`: the threshold rounded down and the cap rounded
    /// up, so the written values keep at least what the replay found.
    pub fn gate(&self) -> Gate {
        Gate {
            threshold: round_down(self.chosen().threshold),
            cap: self.chosen().cap.ceil(),
        }
    }
}

/// Replays `catches` against `checks`, the hub's records of every regression check. `None`
/// when no catch could be scored.
pub fn replay(catches: &[Catch], checks: &[GateCheck]) -> Option<Replay> {
    let lists = solo_scores(catches, checks);
    let mut scored: Vec<(f64, &[(f64, f64)])> = Vec::new();
    for (catch, list) in catches.iter().zip(&lists) {
        let score = checks
            .iter()
            .zip(list)
            .find(|(check, _)| check.check.as_str() == catch.check)
            .map(|(_, (score, _))| *score);
        if let Some(score) = score {
            scored.push((score, list));
        }
    }
    if scored.is_empty() {
        return None;
    }
    let mut candidates: Vec<f64> = scored.iter().map(|(score, _)| *score).collect();
    candidates.sort_by(|a, b| b.total_cmp(a));
    let need = ((KEEP * scored.len() as f64).ceil() as usize).clamp(1, scored.len());
    let chosen = candidates[need - 1];
    candidates.dedup();
    // The chosen threshold's index among the distinct candidates.
    let at = candidates
        .iter()
        .filter(|c| c.total_cmp(&chosen).is_gt())
        .count();
    let start = at.saturating_sub(NEARBY);
    let row = |threshold: f64| {
        let kept = scored.iter().filter(|(s, _)| *s >= threshold).count();
        // With no cap, no builds to charge and nothing cached, the gate runs every check
        // scoring at or above the threshold, so its scored cost is the sum of their costs.
        let costs: Vec<f64> = scored
            .iter()
            .map(|(_, list)| {
                list.iter()
                    .filter(|(score, _)| *score >= threshold)
                    .map(|(_, cost)| cost)
                    .sum()
            })
            .collect();
        Row {
            threshold,
            kept,
            cap: quantile(costs, KEEP),
        }
    };
    let nearby: Vec<Row> = candidates[start..(at + NEARBY + 1).min(candidates.len())]
        .iter()
        .map(|&threshold| row(threshold))
        .collect();
    Some(Replay {
        catches: scored.len(),
        unscored: catches.len() - scored.len(),
        nearby,
        chosen: at - start,
    })
}

/// For each catch, the gate's score and cost for each of `checks` (in order), each planned
/// alone, with the catch itself taken out of its own check's record (see [`without_catch`]) and
/// a check the change could not affect left out (score of negative infinity, no cost). Planning all of them together costs time quadratic in their number, and with no
/// builds to charge and no groups (see [`gate_checks`]) a check's score does not depend on
/// the others. A check the gate would not score gets a score of negative infinity.
fn solo_scores(catches: &[Catch], checks: &[GateCheck]) -> Vec<Vec<(f64, f64)>> {
    let everything = Gate {
        threshold: f64::NEG_INFINITY,
        cap: f64::INFINITY,
    };
    let score_one = |catch: &Catch| -> Vec<(f64, f64)> {
        checks
            .iter()
            .map(|check| {
                let own = check.check.as_str() == catch.check;
                if !own && !affected(catch, check) {
                    return (f64::NEG_INFINITY, 0.0);
                }
                let adjusted;
                let check = if own {
                    adjusted = without_catch(check, catch);
                    &adjusted
                } else {
                    check
                };
                let gate = plan_for(&catch.areas, std::slice::from_ref(check), &everything);
                gate.run
                    .iter()
                    .find(|p| p.why == Why::Scored)
                    .and_then(|p| p.score)
                    .map_or((f64::NEG_INFINITY, 0.0), |score| (score, gate.scored_cost))
            })
            .collect()
    };
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let chunk = catches.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = catches
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(score_one).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("a replay thread panicked"))
            .collect()
    })
}

/// Whether a change to the catch's areas could affect `check`: it has a record after changes to
/// one of them.
fn affected(catch: &Catch, check: &GateCheck) -> bool {
    check
        .history
        .as_ref()
        .is_some_and(|h| catch.areas.iter().any(|a| h.areas.contains_key(a)))
}

/// `check` as the gate would have known it before `catch`: one run and one failure fewer in each
/// of the catch's areas.
pub fn without_catch(check: &GateCheck, catch: &Catch) -> GateCheck {
    let mut check = check.clone();
    if let Some(history) = &mut check.history {
        for area in &catch.areas {
            if let Some(record) = history.areas.get_mut(area)
                && record.failures > 0
            {
                record.runs = record.runs.saturating_sub(1);
                record.failures -= 1;
            }
        }
    }
    check
}

/// The gate's plan for a change to `areas`, with every build current and nothing cached.
fn plan_for(areas: &[String], checks: &[GateCheck], gate: &Gate) -> GatePlan {
    let no_builds = BTreeMap::new();
    let input = GateInput {
        areas,
        checks,
        missing_builds: &no_builds,
        landing: None,
        billions_per_cpu_second: BILLIONS_PER_CPU_SECOND,
    };
    plan(&input, gate)
}

/// The nearest-rank quantile `q` of `values`; zero for none.
fn quantile(mut values: Vec<f64>, q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let rank = ((q * values.len() as f64).ceil() as usize).clamp(1, values.len());
    values[rank - 1]
}

/// `value` rounded down to three significant digits.
fn round_down(value: f64) -> f64 {
    if value <= 0.0 || !value.is_finite() {
        return value;
    }
    let step = 10_f64.powf(value.log10().floor() - 2.0);
    let rounded = (value / step).floor() * step;
    // Undo a step lost to the division landing just below a whole number.
    if (value / step).fract() > 1.0 - 1e-9 {
        rounded + step
    } else {
        rounded
    }
}

/// The hub's check records as the gate sees them. Checks in `left_out` families and checks that
/// are switched off are dropped. Group, builds and cache are left empty: the replay knows none
/// of them for past changes.
pub fn gate_checks(replies: &[CheckStatsReply], left_out: &BTreeSet<String>) -> Vec<GateCheck> {
    let cost = |median: Option<f64>, unit: &str| {
        median.map(|m| {
            if unit == "cpu_s" {
                Cost::CpuSeconds(m)
            } else {
                Cost::Billions(m)
            }
        })
    };
    let mut checks = Vec::new();
    for reply in replies.iter().filter(|r| !left_out.contains(&r.family)) {
        let billions: Vec<f64> = reply
            .checks
            .iter()
            .filter_map(|c| cost(c.median_cost, &c.unit))
            .map(|c| c.billions(BILLIONS_PER_CPU_SECOND))
            .collect();
        let family_median_cost = Cost::Billions(median(&billions).unwrap_or(0.0));
        for stats in reply.checks.iter().filter(|c| c.state != CheckState::Off) {
            let history =
                (!stats.areas.is_empty() || stats.median_cost.is_some()).then(|| History {
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
                    median_cost: cost(stats.median_cost, &stats.unit).unwrap_or(family_median_cost),
                });
            checks.push(GateCheck {
                check: stats.check.clone(),
                always: false,
                edited: false,
                cached: false,
                history,
                family_rate: reply.family_rate,
                family_median_cost,
                needs: Vec::new(),
                group: None,
                regression: None,
            });
        }
    }
    checks
}

/// The text and JSON of a replay over the last `days` days.
pub fn report(found: &Replay, days: u32) -> Reply {
    use std::fmt::Write as _;
    let percent = |kept: usize| 100.0 * kept as f64 / found.catches as f64;
    let gate = found.gate();
    let chosen = found.chosen();
    let mut text = format!("{} catches in the last {days} days", found.catches);
    if found.unscored > 0 {
        let _ = write!(
            text,
            " ({} more left out: their checks have no record)",
            found.unscored
        );
    }
    let _ = write!(
        text,
        "\nthreshold {:.5e}: keeps {} of {} ({:.1}%)\n\
         cap {:.0} billion: covers {:.0}% of those gates\n\n\
         threshold      keeps            cap\n",
        chosen.threshold,
        chosen.kept,
        found.catches,
        percent(chosen.kept),
        gate.cap,
        KEEP * 100.0,
    );
    for (index, row) in found.nearby.iter().enumerate() {
        let mark = if index == found.chosen {
            "  <- chosen"
        } else {
            ""
        };
        let _ = writeln!(
            text,
            "{:<12.5e} {:>5} {:>5.1}% {:>9.0} billion{mark}",
            row.threshold,
            row.kept,
            percent(row.kept),
            row.cap.ceil(),
        );
    }
    let _ = write!(
        text,
        "\nfor ignite.toml:\n[gate]\nthreshold = {:.2e}\ncap = {}\n",
        gate.threshold, gate.cap
    );
    let row_json =
        |r: &Row| serde_json::json!({"threshold": r.threshold, "kept": r.kept, "cap": r.cap});
    let mut reply = Reply::text(Exit::Success, text);
    reply.json = Some(serde_json::json!({
        "days": days,
        "catches": found.catches,
        "unscored": found.unscored,
        "threshold": gate.threshold,
        "cap": gate.cap,
        "chosen": row_json(chosen),
        "nearby": found.nearby.iter().map(row_json).collect::<Vec<_>>(),
    }));
    reply
}

pub fn run(command: &GateCommand, dir: &Path) -> Reply {
    let GateCommand::Replay {
        days,
        hub,
        run_log,
        exploration,
    } = command;
    let since = SystemClock.now().plus_seconds(-f64::from(*days) * 86_400.0);
    match load(dir, hub, run_log, exploration, since) {
        Ok((catches, checks)) => match replay(&catches, &checks) {
            Some(found) => report(&found, *days),
            None => Reply::text(
                Exit::Usage,
                format!(
                    "no catch in the last {days} days has a scored check ({} catches found)",
                    catches.len()
                ),
            ),
        },
        Err(message) => Reply::text(Exit::Usage, message),
    }
}

/// The catches since `since` and the hub's regression checks, outside the families the
/// manifest marks exploration or always.
fn load(
    dir: &Path,
    hub: &Path,
    run_log: &Path,
    exploration: &[String],
    since: Timestamp,
) -> Result<(Vec<Catch>, Vec<GateCheck>), String> {
    let root = ignite_git::tracked::toplevel(dir).map_err(|e| e.to_string())?;
    let config_path = root.join(CONFIG_FILE);
    let text = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    let config = Config::parse(CONFIG_FILE, &text).map_err(|e| e.to_string())?;
    let manifest = read_manifest(&root.join(&config.repository.checks))?;
    let left_out: BTreeSet<String> = manifest
        .families
        .iter()
        .filter(|(_, d)| d.family.always || d.family.kind == Kind::Exploration)
        .map(|(name, _)| name.clone())
        .collect();
    let tracked = ignite_git::tracked::files(&root).map_err(|e| e.to_string())?;
    let graph = crate_graph(&root, &tracked)?;
    let git = GitCli {
        repo: root,
        remote: config.repository.remote.clone(),
    };
    let source = RunLogSource {
        folder: run_log.to_owned(),
        git: &git,
        graph,
        staging: config.repository.staging.clone(),
        exploration: exploration.to_vec(),
    };
    let conn = ignite_store::hub::open(hub).map_err(|e| e.to_string())?;
    let (catches, _) = runlog::catches(&conn, &source, since).map_err(|e| e.to_string())?;
    let families =
        ignite_store::hub::checks::regression_families(&conn).map_err(|e| e.to_string())?;
    let mut replies = Vec::with_capacity(families.len());
    for family in &families {
        replies.push(ignite_store::hub::checks::stats(&conn, family).map_err(|e| e.to_string())?);
    }
    Ok((catches, gate_checks(&replies, &left_out)))
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
