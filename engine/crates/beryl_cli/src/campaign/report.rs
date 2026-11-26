//! Campaign plain-text report (`report.txt`), at most about 120 lines so it can be pasted as a
//! ticket comment.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::plan::StageKind;
use super::stage::positions_count;
use super::{CampaignMeta, StageStatus, StateEvent};
use crate::blunders;
use crate::ladder::{self, Verdict};
use crate::match_log::{DecisionRecord, GameRecord, Verdict as GameVerdict, files};
use crate::value::report::Report as ValueReport;

/// Renders `report.txt` from the campaign's metadata, state log, and stage directories.
pub fn render(
    dir: &Path,
    meta: &CampaignMeta,
    events: &[StateEvent],
    now: u64,
) -> Result<String, String> {
    let mut latest: BTreeMap<&str, &StateEvent> = BTreeMap::new();
    for e in events {
        latest.insert(e.stage.as_str(), e);
    }

    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let hours = now.saturating_sub(meta.started_at) as f64 / 3600.0;

    let mut out = String::new();
    let mode = if meta.smoke { " (smoke)" } else { "" };
    let _ = writeln!(out, "Campaign: {}{mode}", meta.plan.name);
    let commit = if meta.commit.is_empty() {
        "unknown"
    } else {
        meta.commit.as_str()
    };
    let _ = writeln!(out, "Commit: {commit}  Threads: {threads}");
    let _ = writeln!(out, "Hours: {hours:.2}");

    // Positions summary (if the plan has any positions stage).
    for stage in &meta.plan.stages {
        if matches!(stage.kind, StageKind::Positions { .. }) {
            let count = positions_count(&dir.join(&stage.id))?;
            if count > 0 {
                let _ = writeln!(out, "Positions ({}): {count} kept", stage.id);
            } else {
                let _ = writeln!(
                    out,
                    "Positions ({}): none (matches ran without --positions)",
                    stage.id
                );
            }
        }
    }

    render_gates(dir, meta, &mut out)?;
    render_matches(dir, meta, events, &mut out)?;
    render_value_tests(dir, meta, &mut out)?;
    render_failed_and_skipped(meta, &latest, &mut out);

    Ok(out)
}

fn render_gates(dir: &Path, meta: &CampaignMeta, out: &mut String) -> Result<(), String> {
    let mut gate_lines = Vec::new();
    for stage in &meta.plan.stages {
        if !matches!(stage.kind, StageKind::Gate { .. }) {
            continue;
        }
        let stage_dir = dir.join(&stage.id);
        let champ_path = stage_dir.join("champion.json");
        let chall_path = stage_dir.join("challenger.json");
        if !champ_path.exists() || !chall_path.exists() {
            continue;
        }
        let champ: blunders::Report = read_json(&champ_path)?;
        let chall: blunders::Report = read_json(&chall_path)?;
        gate_lines.push(format_gate(&stage.id, &champ, &chall));
    }
    if !gate_lines.is_empty() {
        let _ = writeln!(out, "\nGates:");
        for block in gate_lines {
            out.push_str(&block);
        }
    }
    Ok(())
}

fn render_matches(
    dir: &Path,
    meta: &CampaignMeta,
    events: &[StateEvent],
    out: &mut String,
) -> Result<(), String> {
    let mut seen_roots = BTreeSet::new();
    let mut match_blocks = Vec::new();
    for stage in &meta.plan.stages {
        if !stage.is_match() {
            continue;
        }
        let Some(m) = meta.plan.resolved_match(&stage.id) else {
            continue;
        };
        if !seen_roots.insert(m.root_id.clone()) {
            continue;
        }
        // Highest cap among stages sharing this root that ever started or finished, so a match
        // extension killed at the deadline or failed after making progress still reports all its
        // counted pairs.
        let mut cap = m.cap;
        for other in &meta.plan.stages {
            if let Some(om) = meta.plan.resolved_match(&other.id)
                && om.root_id == m.root_id
                && events.iter().any(|e| {
                    e.stage == other.id
                        && matches!(e.status, StageStatus::Started | StageStatus::Finished)
                })
            {
                cap = cap.max(om.cap);
            }
        }
        let match_dir = dir.join(&m.root_id);
        if let Some(report) = ladder::summarize(&match_dir, cap)? {
            match_blocks.push(format_match(&m.root_id, &match_dir, &report)?);
        }
    }
    if !match_blocks.is_empty() {
        let _ = writeln!(out, "\nMatches:");
        for block in match_blocks {
            out.push_str(&block);
        }
    }
    Ok(())
}

fn render_value_tests(dir: &Path, meta: &CampaignMeta, out: &mut String) -> Result<(), String> {
    for stage in &meta.plan.stages {
        if !stage.is_value_test() {
            continue;
        }
        let path = dir.join(&stage.id).join("value.json");
        if !path.exists() {
            continue;
        }
        let report: ValueReport = read_json(&path)?;
        let _ = writeln!(out, "\nValue test ({}):", stage.id);
        out.push_str(&report.text());
    }
    Ok(())
}

fn render_failed_and_skipped(
    meta: &CampaignMeta,
    latest: &BTreeMap<&str, &StateEvent>,
    out: &mut String,
) {
    let failed: Vec<(&str, &StateEvent)> = meta
        .plan
        .stages
        .iter()
        .filter_map(|s| {
            let e = latest.get(s.id.as_str())?;
            (e.status == StageStatus::Failed).then_some((s.id.as_str(), *e))
        })
        .collect();
    if !failed.is_empty() {
        let _ = writeln!(out, "\nFailed:");
        for (id, e) in failed {
            let line = e
                .stderr
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(String::as_str)
                .or(e.reason.as_deref())
                .unwrap_or("unknown error");
            let _ = writeln!(out, "  {id}: {line}");
        }
    }

    let skipped: Vec<(&str, &StateEvent)> = meta
        .plan
        .stages
        .iter()
        .filter_map(|s| {
            let e = latest.get(s.id.as_str())?;
            (e.status == StageStatus::Skipped).then_some((s.id.as_str(), *e))
        })
        .collect();
    if !skipped.is_empty() {
        let _ = writeln!(out, "\nSkipped:");
        for (id, e) in skipped {
            let why = e.reason.as_deref().unwrap_or("skipped");
            let _ = writeln!(out, "  {id}: {why}");
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn format_gate(id: &str, champion: &blunders::Report, challenger: &blunders::Report) -> String {
    let mut out = String::new();
    let runs = champion.budgets.len() * champion.seeds as usize;
    let total = champion.entries.len() * runs;
    let mut champ_passes = 0usize;
    let mut chall_passes = 0usize;
    let mut diffs = Vec::new();
    for (before, after) in champion.entries.iter().zip(&challenger.entries) {
        let b = before.runs.iter().filter(|r| r.pass).count();
        let a = after.runs.iter().filter(|r| r.pass).count();
        champ_passes += b;
        chall_passes += a;
        let d = b.abs_diff(a);
        if d >= 6 {
            let sign = if a >= b { '+' } else { '-' };
            diffs.push(format!(
                "  {}: {b}/{runs} -> {a}/{runs} ({sign}{d})",
                before.name
            ));
        }
    }
    let _ = writeln!(
        out,
        "  {id}: champion {champ_passes}/{total} ({} fixed), challenger {chall_passes}/{total} ({} fixed)",
        champion.fixed, challenger.fixed
    );
    for line in diffs {
        let _ = writeln!(out, "  {line}");
    }
    out
}

fn format_match(id: &str, match_dir: &Path, report: &ladder::Report) -> Result<String, String> {
    let mut out = String::new();
    let _ = writeln!(out, "{}", report.row());
    let verdict = match report.verdict {
        Verdict::Pass => "pass",
        Verdict::Fail => "fail",
        Verdict::Undecided => "undecided",
        Verdict::Audit => "audit",
    };
    let _ = write!(
        out,
        "  {id}: games {} (cap {}), score {:.1}% [{:.1}, {:.1}], Elo {} [{}, {}], verdict {verdict}, crashed {}",
        report.games,
        report.cap,
        100.0 * report.score,
        100.0 * report.score_interval[0],
        100.0 * report.score_interval[1],
        signed(report.elo),
        signed(report.elo_interval[0]),
        signed(report.elo_interval[1]),
        report.crashed.len()
    );
    if !report.crashed.is_empty() {
        let names: Vec<&str> = report.crashed.keys().map(String::as_str).collect();
        let _ = write!(out, " ({})", names.join(", "));
    }
    let _ = writeln!(out);

    let stats = entrant_stats(match_dir)?;
    for (name, stat) in [(&report.challenger, stats[0]), (&report.champion, stats[1])] {
        let per_game = if stat.games > 0 {
            stat.searched as f64 / stat.games as f64
        } else {
            0.0
        };
        let overrule = if stat.anchored > 0 {
            format!(
                "{:.1}% ({}/{})",
                100.0 * stat.overruled as f64 / stat.anchored as f64,
                stat.overruled,
                stat.anchored
            )
        } else {
            "n/a".to_owned()
        };
        let _ = writeln!(
            out,
            "  {name}: {per_game:.1} searched/game, overruled {overrule}"
        );
    }
    Ok(out)
}

#[derive(Clone, Copy, Default)]
struct EntrantStat {
    games: u64,
    searched: u64,
    anchored: u64,
    overruled: u64,
}

/// Computes searched decisions per game and anchor overrule share for `[challenger, champion]`
/// from `games.jsonl` and `decisions.jsonl`.
fn entrant_stats(match_dir: &Path) -> Result<[EntrantStat; 2], String> {
    let games = files::read::<GameRecord>(&match_dir.join("games.jsonl"))?;
    let mut seat_of: BTreeMap<String, u8> = BTreeMap::new();
    let mut non_crashed = 0u64;
    for g in games {
        if g.outcome != GameVerdict::Crashed {
            non_crashed += 1;
            seat_of.insert(g.game, g.seat);
        }
    }
    let mut stats = [EntrantStat {
        games: non_crashed,
        ..EntrantStat::default()
    }; 2];
    let decisions = files::read::<DecisionRecord>(&match_dir.join("decisions.jsonl"))?;
    for d in decisions {
        let Some(&challenger_seat) = seat_of.get(&d.game) else {
            continue;
        };
        let side = usize::from(d.seat != challenger_seat);
        stats[side].searched += 1;
        if let Some(anchor) = d.anchor {
            stats[side].anchored += 1;
            if d.choice != anchor.greedy {
                stats[side].overruled += 1;
            }
        }
    }
    Ok(stats)
}

fn signed(elo: f64) -> String {
    if elo.is_finite() {
        format!("{elo:+.0}")
    } else if elo > 0.0 {
        "+inf".to_owned()
    } else {
        "-inf".to_owned()
    }
}
