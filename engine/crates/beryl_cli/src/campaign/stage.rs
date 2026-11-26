//! Running one campaign stage as a child process of `std::env::current_exe()`.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::plan::{Plan, Stage, StageKind};
use crate::ladder::{self, Verdict};
use crate::match_log::{GameRecord, files};

/// How often the runner checks on a stage's child process.
const POLL: Duration = Duration::from_millis(50);

/// How often the runner prints a progress line while a stage runs.
const PROGRESS_EVERY: Duration = Duration::from_hours(1);

/// How many trailing stderr lines are recorded when a stage fails.
const STDERR_LINES: usize = 20;

/// How many consecutive failed attempts with no progress fail a stage.
const MAX_NO_PROGRESS: u32 = 3;

/// The outcome of attempting to run one stage.
#[derive(Clone, Debug)]
pub enum Outcome {
    Finished,
    Failed { reason: String, stderr: Vec<String> },
    Skipped { reason: String },
}

enum Invocation {
    Skip(String),
    Run {
        args: Vec<String>,
        stage_dir: PathBuf,
        match_dir: Option<PathBuf>,
    },
}

/// Builds the child-process arguments and target directories for `stage`, or returns
/// `Invocation::Skip` when the stage should be skipped before spawning.
fn prepare(
    stage: &Stage,
    plan: &Plan,
    dir: &Path,
    positions_dir: Option<&Path>,
) -> Result<Invocation, String> {
    match &stage.kind {
        StageKind::Positions { games, seed, work } => {
            let out = dir.join(&stage.id);
            let args = vec![
                "positions".to_owned(),
                "build".to_owned(),
                "--games".to_owned(),
                games.to_string(),
                "--seed".to_owned(),
                seed.to_string(),
                "--work".to_owned(),
                work.as_arg(),
                "--out".to_owned(),
                out.to_string_lossy().into_owned(),
            ];
            Ok(Invocation::Run {
                args,
                stage_dir: out,
                match_dir: None,
            })
        }
        StageKind::Gate {
            challenger,
            champion,
            claims,
            seeds,
            work,
        } => {
            let out = dir.join(&stage.id);
            let mut args = vec![
                "gate".to_owned(),
                challenger.clone(),
                "--claims".to_owned(),
                claims.clone().unwrap_or_else(|| "none".to_owned()),
            ];
            if let Some(champ) = champion {
                args.push("--vs".to_owned());
                args.push(champ.clone());
            }
            args.push("--out".to_owned());
            args.push(out.to_string_lossy().into_owned());
            if let Some(s) = seeds {
                args.push("--seeds".to_owned());
                args.push(s.to_string());
            }
            if let Some(w) = work {
                args.push("--work".to_owned());
                args.push(w.clone());
            }
            Ok(Invocation::Run {
                args,
                stage_dir: out,
                match_dir: None,
            })
        }
        StageKind::Match { .. } => prepare_match(stage, plan, dir, positions_dir),
        StageKind::ValueTest { matches, seed } => {
            prepare_value_test(&stage.id, plan, dir, matches, *seed)
        }
    }
}

fn prepare_match(
    stage: &Stage,
    plan: &Plan,
    dir: &Path,
    positions_dir: Option<&Path>,
) -> Result<Invocation, String> {
    let m = plan
        .resolved_match(&stage.id)
        .ok_or_else(|| format!("stage {}: not a match stage", stage.id))?;
    let out = dir.join(&m.root_id);
    if let Some(ref base_id) = m.extends
        && let Some(base) = plan.resolved_match(base_id)
        && let Some(report) = ladder::summarize(&out, base.cap)?
        && matches!(report.verdict, Verdict::Pass | Verdict::Fail)
    {
        let v = match report.verdict {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            _ => unreachable!(),
        };
        return Ok(Invocation::Skip(format!("{base_id} already decided ({v})")));
    }
    let mut args = vec![
        "ladder".to_owned(),
        m.challenger,
        "--vs".to_owned(),
        m.champion,
        "--work".to_owned(),
        m.work,
        "--bar".to_owned(),
        m.bar,
        "--cap".to_owned(),
        m.cap.to_string(),
        "--seed".to_owned(),
        m.seed.to_string(),
        "--out".to_owned(),
        out.to_string_lossy().into_owned(),
    ];
    if let Some(pos) = positions_dir {
        args.push("--positions".to_owned());
        args.push(pos.to_string_lossy().into_owned());
    }
    Ok(Invocation::Run {
        args,
        stage_dir: out.clone(),
        match_dir: Some(out),
    })
}

fn prepare_value_test(
    stage_id: &str,
    plan: &Plan,
    dir: &Path,
    matches: &[String],
    seed: u64,
) -> Result<Invocation, String> {
    let out = dir.join(stage_id);
    let mut visited = BTreeSet::new();
    let mut match_dirs: Vec<PathBuf> = Vec::new();
    for m_id in matches {
        let Some(m) = plan.resolved_match(m_id) else {
            continue;
        };
        if !visited.insert(m.root_id.clone()) {
            continue;
        }
        let m_dir = dir.join(&m.root_id);
        if game_count(&m_dir)? > 0 {
            match_dirs.push(m_dir);
        }
    }
    if match_dirs.is_empty() {
        return Ok(Invocation::Skip("no match stage has games".to_owned()));
    }
    let mut args = vec!["value-test".to_owned()];
    for m_dir in &match_dirs {
        args.push(m_dir.to_string_lossy().into_owned());
    }
    args.push("--seed".to_owned());
    args.push(seed.to_string());
    args.push("--out".to_owned());
    args.push(out.to_string_lossy().into_owned());
    Ok(Invocation::Run {
        args,
        stage_dir: out,
        match_dir: None,
    })
}

/// Runs `stage` of `plan` in campaign directory `dir` as a child process of `exe`.
pub fn run(
    exe: &Path,
    stage: &Stage,
    plan: &Plan,
    dir: &Path,
    positions_dir: Option<&Path>,
    deadline_at: u64,
    now_secs: fn() -> u64,
) -> Result<Outcome, String> {
    if !stage.is_value_test() && now_secs() >= deadline_at {
        return Ok(Outcome::Skipped {
            reason: "time limit reached".to_owned(),
        });
    }

    let (args, stage_dir, match_dir) = match prepare(stage, plan, dir, positions_dir)? {
        Invocation::Skip(reason) => return Ok(Outcome::Skipped { reason }),
        Invocation::Run {
            args,
            stage_dir,
            match_dir,
        } => (args, stage_dir, match_dir),
    };

    let stderr_path = dir.join(format!(".{}.stderr", stage.id));
    let mut no_progress = 0u32;
    let mut last_hourly = Instant::now();
    loop {
        if !stage.is_value_test() && now_secs() >= deadline_at {
            return Ok(Outcome::Skipped {
                reason: "time limit reached".to_owned(),
            });
        }
        // If a previous runner was killed mid-stage, an orphan child may still hold the stage
        // directory's lock. Wait for it to release rather than burning retries immediately.
        if stage_dir.join("lock").exists()
            && !wait_for_lock(
                &stage_dir,
                stage,
                match_dir.as_deref(),
                deadline_at,
                now_secs,
                &mut last_hourly,
            )?
        {
            return Ok(Outcome::Skipped {
                reason: "time limit reached".to_owned(),
            });
        }

        let games_before = if let Some(m_dir) = &match_dir {
            game_count(m_dir)?
        } else {
            empty_dir(&stage_dir)?;
            0
        };

        let stderr_file =
            File::create(&stderr_path).map_err(|e| format!("{}: {e}", stderr_path.display()))?;
        let mut child = Command::new(exe)
            .args(&args)
            .env("BERYL_CAMPAIGN_STAGE", &stage.id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .map_err(|e| format!("starting {}: {e}", exe.display()))?;

        let status = wait_for_child(
            &mut child,
            stage,
            match_dir.as_deref(),
            deadline_at,
            now_secs,
            &mut last_hourly,
        )?;

        let Some(status) = status else {
            let _ = fs::remove_file(&stderr_path);
            return Ok(Outcome::Skipped {
                reason: "time limit reached".to_owned(),
            });
        };

        let stderr_tail = read_tail_lines(&stderr_path, STDERR_LINES);
        let _ = fs::remove_file(&stderr_path);

        if status.success() {
            return Ok(Outcome::Finished);
        }

        let made_progress = match &match_dir {
            Some(m_dir) => game_count(m_dir)? > games_before,
            None => false,
        };
        if made_progress {
            no_progress = 0;
        } else {
            no_progress += 1;
        }
        if no_progress >= MAX_NO_PROGRESS {
            return Ok(Outcome::Failed {
                reason: format!("exited with {status}"),
                stderr: stderr_tail,
            });
        }
        eprintln!(
            "campaign: stage {} failed ({status}); retrying ({no_progress}/{MAX_NO_PROGRESS})",
            stage.id
        );
    }
}

/// Waits for `child` to exit, killing it if `deadline_at` is reached on a non-`value-test` stage.
fn wait_for_child(
    child: &mut std::process::Child,
    stage: &Stage,
    match_dir: Option<&Path>,
    deadline_at: u64,
    now_secs: fn() -> u64,
    last_hourly: &mut Instant,
) -> Result<Option<std::process::ExitStatus>, String> {
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("waiting for stage {}: {e}", stage.id))?
        {
            return Ok(Some(status));
        }
        if !stage.is_value_test() && now_secs() >= deadline_at {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        if last_hourly.elapsed() >= PROGRESS_EVERY {
            print_hourly(&stage.id, match_dir, deadline_at, now_secs);
            *last_hourly = Instant::now();
        }
        thread::sleep(POLL);
    }
}

/// Waits until no orphan process holds `stage_dir/lock`. Returns `false` if the deadline is
/// reached first.
fn wait_for_lock(
    stage_dir: &Path,
    stage: &Stage,
    match_dir: Option<&Path>,
    deadline_at: u64,
    now_secs: fn() -> u64,
    last_hourly: &mut Instant,
) -> Result<bool, String> {
    let lock_path = stage_dir.join("lock");
    loop {
        if let Ok(lock) = files::lock_file(&lock_path)? {
            drop(lock);
            return Ok(true);
        }
        if !stage.is_value_test() && now_secs() >= deadline_at {
            return Ok(false);
        }
        if last_hourly.elapsed() >= PROGRESS_EVERY {
            print_hourly(&stage.id, match_dir, deadline_at, now_secs);
            *last_hourly = Instant::now();
        }
        thread::sleep(POLL);
    }
}

fn print_hourly(stage_id: &str, match_dir: Option<&Path>, deadline_at: u64, now_secs: fn() -> u64) {
    let pairs = match match_dir {
        Some(m_dir) => pair_count(m_dir),
        None => 0,
    };
    let left_hours = deadline_at.saturating_sub(now_secs()) as f64 / 3600.0;
    eprintln!("campaign: stage {stage_id} running, {pairs} pairs counted, {left_hours:.1}h left");
}

/// Reads the last `max_lines` lines from `path`, returning an empty list if unreadable.
fn read_tail_lines(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].iter().map(|s| (*s).to_owned()).collect()
}

/// Empties `dir` (if it exists) and recreates it, propagating any disk or permission error with
/// the path.
fn empty_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// Number of finished games in `match_dir/games.jsonl`.
pub(super) fn game_count(match_dir: &Path) -> Result<usize, String> {
    Ok(files::read::<GameRecord>(&match_dir.join("games.jsonl"))?.len())
}

/// Number of completed pairs in `match_dir/games.jsonl` (best effort, for hourly progress).
fn pair_count(match_dir: &Path) -> usize {
    let Ok(records) = files::read::<GameRecord>(&match_dir.join("games.jsonl")) else {
        return 0;
    };
    let mut seats = BTreeSet::new();
    let mut pairs = 0usize;
    for r in records {
        if seats.insert((r.pair, r.seat)) && seats.contains(&(r.pair, 1 - r.seat)) {
            pairs += 1;
        }
    }
    pairs
}

/// Number of `.json` position files in `dir`, or 0 if `dir` does not exist.
pub(super) fn positions_count(dir: &Path) -> Result<usize, String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            count += 1;
        }
    }
    Ok(count)
}
