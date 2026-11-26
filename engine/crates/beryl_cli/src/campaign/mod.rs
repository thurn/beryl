//! `beryl_cli campaign <plan.json> [--hours N] [--smoke] [--commit HASH] [--accept-commit]`
//! (`just campaign`): plays a plan's stages in order as child processes, survives crashes, kills,
//! and power cuts, and writes one plain-text report (`report.txt`).

pub mod plan;
mod report;
mod stage;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use plan::Plan;
use stage::{Outcome, positions_count};

use crate::match_log::cache;
use crate::match_log::files::{self, Log};

const DEFAULT_HOURS: f64 = 168.0;

const USAGE: &str = "usage: beryl_cli campaign <plan.json> [--hours N] [--smoke] \
                     [--commit HASH] [--accept-commit]";

/// `campaign.json` in the campaign directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignMeta {
    pub plan: Plan,
    pub commit: String,
    pub started_at: u64,
    pub deadline_at: u64,
    pub smoke: bool,
}

/// One line of `state.jsonl`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateEvent {
    pub stage: String,
    pub status: StageStatus,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stderr: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageStatus {
    Started,
    Finished,
    Failed,
    Skipped,
}

struct Options {
    plan_path: PathBuf,
    hours: Option<f64>,
    smoke: bool,
    commit: Option<String>,
    accept_commit: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut plan_path = None;
        let mut hours = None;
        let mut smoke = false;
        let mut commit = None;
        let mut accept_commit = false;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .cloned()
                    .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
            };
            match arg.as_str() {
                "--hours" => {
                    let v = value()?;
                    let h: f64 = v
                        .parse()
                        .map_err(|_| format!("--hours: not a number: {v:?}"))?;
                    if !h.is_finite() || h < 0.0 {
                        return Err(format!("--hours: must be finite and non-negative: {v:?}"));
                    }
                    hours = Some(h);
                }
                "--smoke" => smoke = true,
                "--commit" => commit = Some(value()?),
                "--accept-commit" => accept_commit = true,
                flag if flag.starts_with("--") => {
                    return Err(format!("unknown flag {flag}\n{USAGE}"));
                }
                path if plan_path.is_none() => plan_path = Some(PathBuf::from(path)),
                _ => return Err(USAGE.to_owned()),
            }
        }
        let plan_path = plan_path.ok_or_else(|| USAGE.to_owned())?;
        Ok(Self {
            plan_path,
            hours,
            smoke,
            commit,
            accept_commit,
        })
    }
}

pub fn main(args: &[String]) -> Result<(), String> {
    let opts = Options::parse(args)?;
    let plan = Plan::load(&opts.plan_path)?;
    let exe = std::env::current_exe().map_err(|e| format!("finding this binary: {e}"))?;

    let base_dir = cache().join("campaign");
    if opts.smoke {
        let smoke_plan = plan.smoke();
        let smoke_dir = base_dir.join(format!("{}-smoke", plan.name));
        fs::create_dir_all(&smoke_dir).map_err(|e| format!("{}: {e}", smoke_dir.display()))?;
        let _lock = files::lock(&smoke_dir)?;
        let text = run_campaign(
            &exe,
            &smoke_plan,
            &smoke_dir,
            opts.hours,
            true,
            opts.commit.as_deref(),
            opts.accept_commit,
        )?;
        print!("{text}");
        return Ok(());
    }

    let real_dir = base_dir.join(&plan.name);
    fs::create_dir_all(&real_dir).map_err(|e| format!("{}: {e}", real_dir.display()))?;
    let _lock = files::lock(&real_dir)?;

    // Validate an existing real campaign's commit and deadline before running preflight, so a
    // commit mismatch fails immediately and an already-expired run skips preflight.
    let skip_preflight = check_real_before_preflight(
        &real_dir,
        opts.hours,
        opts.commit.as_deref(),
        opts.accept_commit,
    )?;
    if !skip_preflight {
        run_preflight(&exe, &plan, &base_dir, opts.commit.as_deref())?;
    }

    let text = run_campaign(
        &exe,
        &plan,
        &real_dir,
        opts.hours,
        false,
        opts.commit.as_deref(),
        opts.accept_commit,
    )?;
    print!("{text}");
    Ok(())
}

/// Checks an existing real campaign's commit mismatch before preflight, and returns `true` if
/// the real campaign is already at or past its deadline so preflight should be skipped.
fn check_real_before_preflight(
    real_dir: &Path,
    hours: Option<f64>,
    commit: Option<&str>,
    accept_commit: bool,
) -> Result<bool, String> {
    let meta_path = real_dir.join("campaign.json");
    match fs::read_to_string(&meta_path) {
        Ok(text) => {
            let meta: CampaignMeta =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", meta_path.display()))?;
            if let Some(c) = commit
                && !c.is_empty()
                && !meta.commit.is_empty()
                && meta.commit != c
                && !accept_commit
            {
                return Err(format!(
                    "campaign {} was started at commit {}, not {c}: pass --accept-commit to continue",
                    meta.plan.name, meta.commit
                ));
            }
            let deadline = match hours {
                Some(h) => meta.started_at.saturating_add(hours_to_secs(h)),
                None => meta.deadline_at,
            };
            Ok(now_secs() >= deadline)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(hours == Some(0.0)),
        Err(e) => Err(format!("{}: {e}", meta_path.display())),
    }
}

/// Runs the smoke plan in `<name>-smoke` unless it already finished at `commit` with no failed
/// stages. If any smoke stage fails, prints the failure and returns `Err`.
fn run_preflight(
    exe: &Path,
    plan: &Plan,
    base_dir: &Path,
    commit: Option<&str>,
) -> Result<(), String> {
    let smoke_plan = plan.smoke();
    let smoke_dir = base_dir.join(format!("{}-smoke", plan.name));
    fs::create_dir_all(&smoke_dir).map_err(|e| format!("{}: {e}", smoke_dir.display()))?;
    let _smoke_lock = files::lock(&smoke_dir)?;

    if smoke_finished_at(&smoke_dir, &smoke_plan, commit)? {
        eprintln!(
            "campaign: preflight already finished in {}",
            smoke_dir.display()
        );
        return Ok(());
    }
    // If a smoke directory exists from another commit, clear its contents (keeping the lock
    // files) so preflight runs fresh on this commit.
    let meta_path = smoke_dir.join("campaign.json");
    if meta_path.exists()
        && let Ok(text) = fs::read_to_string(&meta_path)
        && let Ok(meta) = serde_json::from_str::<CampaignMeta>(&text)
        && commit.is_some_and(|c| !c.is_empty() && !meta.commit.is_empty() && meta.commit != c)
    {
        clear_keeping_lock(&smoke_dir)?;
    }
    eprintln!("campaign: running preflight in {}", smoke_dir.display());
    let _ = run_campaign(exe, &smoke_plan, &smoke_dir, None, true, commit, true)?;
    let events: Vec<StateEvent> = files::read(&smoke_dir.join("state.jsonl"))?;
    let latest = latest_by_stage(&events);
    let failed: Vec<String> = smoke_plan
        .stages
        .iter()
        .filter_map(|s| {
            let e = latest.get(s.id.as_str())?;
            if e.status != StageStatus::Failed {
                return None;
            }
            let msg = e
                .stderr
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(String::as_str)
                .or(e.reason.as_deref())
                .unwrap_or("failed");
            Some(format!("{}: {msg}", s.id))
        })
        .collect();
    if !failed.is_empty() {
        return Err(format!(
            "preflight failed; real run did not start:\n{}",
            failed.join("\n")
        ));
    }
    Ok(())
}

/// Removes everything inside `dir` except the lock files so the held lock remains valid.
fn clear_keeping_lock(dir: &Path) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = entry.file_name();
        if name == "lock" || name == "lock.guard" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Whether `smoke_dir` already shows `smoke_plan` finished at `commit` with no failed or
/// time-skipped stages.
fn smoke_finished_at(dir: &Path, plan: &Plan, commit: Option<&str>) -> Result<bool, String> {
    let meta_path = dir.join("campaign.json");
    let text = match fs::read_to_string(&meta_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{}: {e}", meta_path.display())),
    };
    let meta: CampaignMeta =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", meta_path.display()))?;
    if let Some(c) = commit
        && !c.is_empty()
        && meta.commit != c
    {
        return Ok(false);
    }
    let events: Vec<StateEvent> = files::read(&dir.join("state.jsonl"))?;
    let latest = latest_by_stage(&events);
    Ok(plan.stages.iter().all(|s| {
        latest.get(s.id.as_str()).is_some_and(|e| {
            e.status == StageStatus::Finished
                || (e.status == StageStatus::Skipped
                    && e.reason
                        .as_deref()
                        .is_some_and(|r| r.contains("already decided")))
        })
    }))
}

fn run_campaign(
    exe: &Path,
    plan: &Plan,
    dir: &Path,
    hours: Option<f64>,
    smoke: bool,
    commit: Option<&str>,
    accept_commit: bool,
) -> Result<String, String> {
    let meta = open_or_create_meta(dir, plan, hours, smoke, commit, accept_commit)?;
    let state_path = dir.join("state.jsonl");
    let mut events: Vec<StateEvent> = files::read(&state_path)?;
    let mut log = Log::open(&state_path)?;

    for stage in &plan.stages {
        let prior = latest_status(&events, &stage.id);
        if prior == Some(StageStatus::Finished) {
            continue;
        }
        if prior == Some(StageStatus::Failed) && !accept_commit {
            continue;
        }

        let positions_dir = if stage.is_match() {
            match plan.positions_before(&stage.id) {
                Some(pos_id) => {
                    let p = dir.join(pos_id);
                    (positions_count(&p)? > 0).then_some(p)
                }
                None => None,
            }
        } else {
            None
        };

        // Only log `started` if we are not already past the deadline (or this is value-test).
        if !stage.is_value_test() && now_secs() >= meta.deadline_at {
            let ev = StateEvent {
                stage: stage.id.clone(),
                status: StageStatus::Skipped,
                at: now_secs(),
                reason: Some("time limit reached".to_owned()),
                stderr: Vec::new(),
            };
            log.append(&ev)?;
            events.push(ev);
            eprintln!("campaign: stage {} skipped: time limit reached", stage.id);
            write_report(dir, &meta, &events)?;
            continue;
        }

        eprintln!(
            "campaign: stage {} ({}) started",
            stage.id,
            stage.kind_name()
        );
        let started = StateEvent {
            stage: stage.id.clone(),
            status: StageStatus::Started,
            at: now_secs(),
            reason: None,
            stderr: Vec::new(),
        };
        log.append(&started)?;
        events.push(started);

        let outcome = stage::run(
            exe,
            stage,
            plan,
            dir,
            positions_dir.as_deref(),
            meta.deadline_at,
            now_secs,
        )?;

        let end = event_from_outcome(&stage.id, outcome);
        log.append(&end)?;
        events.push(end);
        write_report(dir, &meta, &events)?;
    }

    write_report(dir, &meta, &events)
}

fn event_from_outcome(stage_id: &str, outcome: Outcome) -> StateEvent {
    match outcome {
        Outcome::Finished => {
            eprintln!("campaign: stage {stage_id} finished");
            StateEvent {
                stage: stage_id.to_owned(),
                status: StageStatus::Finished,
                at: now_secs(),
                reason: None,
                stderr: Vec::new(),
            }
        }
        Outcome::Failed { reason, stderr } => {
            let last = stderr
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map_or(reason.as_str(), String::as_str);
            eprintln!("campaign: stage {stage_id} failed: {last}");
            StateEvent {
                stage: stage_id.to_owned(),
                status: StageStatus::Failed,
                at: now_secs(),
                reason: Some(reason),
                stderr,
            }
        }
        Outcome::Skipped { reason } => {
            eprintln!("campaign: stage {stage_id} skipped: {reason}");
            StateEvent {
                stage: stage_id.to_owned(),
                status: StageStatus::Skipped,
                at: now_secs(),
                reason: Some(reason),
                stderr: Vec::new(),
            }
        }
    }
}

fn open_or_create_meta(
    dir: &Path,
    plan: &Plan,
    hours: Option<f64>,
    smoke: bool,
    commit: Option<&str>,
    accept_commit: bool,
) -> Result<CampaignMeta, String> {
    let path = dir.join("campaign.json");
    match fs::read_to_string(&path) {
        Ok(text) => {
            let mut meta: CampaignMeta =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut changed = false;
            if let Some(c) = commit
                && !c.is_empty()
            {
                if meta.commit.is_empty() {
                    c.clone_into(&mut meta.commit);
                    changed = true;
                } else if meta.commit != c {
                    if accept_commit {
                        c.clone_into(&mut meta.commit);
                        changed = true;
                    } else {
                        return Err(format!(
                            "campaign {} was started at commit {}, not {c}: pass --accept-commit to continue",
                            meta.plan.name, meta.commit
                        ));
                    }
                }
            }
            if let Some(h) = hours {
                meta.deadline_at = meta.started_at.saturating_add(hours_to_secs(h));
                changed = true;
            }
            if changed {
                save_meta(&path, &meta)?;
            }
            Ok(meta)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let started_at = now_secs();
            let h = hours.unwrap_or(DEFAULT_HOURS);
            let meta = CampaignMeta {
                plan: plan.clone(),
                commit: commit.unwrap_or_default().to_owned(),
                started_at,
                deadline_at: started_at.saturating_add(hours_to_secs(h)),
                smoke,
            };
            save_meta(&path, &meta)?;
            Ok(meta)
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn save_meta(path: &Path, meta: &CampaignMeta) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())? + "\n";
    files::write_whole(path, json.as_bytes())
}

fn write_report(dir: &Path, meta: &CampaignMeta, events: &[StateEvent]) -> Result<String, String> {
    let text = report::render(dir, meta, events, now_secs())?;
    files::write_whole(&dir.join("report.txt"), text.as_bytes())?;
    Ok(text)
}

fn latest_by_stage(events: &[StateEvent]) -> BTreeMap<&str, &StateEvent> {
    let mut map = BTreeMap::new();
    for e in events {
        map.insert(e.stage.as_str(), e);
    }
    map
}

fn latest_status(events: &[StateEvent], stage_id: &str) -> Option<StageStatus> {
    events
        .iter()
        .rev()
        .find(|e| e.stage == stage_id)
        .map(|e| e.status)
}

fn hours_to_secs(hours: f64) -> u64 {
    (hours * 3600.0).round() as u64
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
