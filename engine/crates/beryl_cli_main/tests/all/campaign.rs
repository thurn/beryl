//! `beryl_cli campaign`: runs a three-stage plan (a gate, a match of `greedy` against
//! `greedy+hold` at cap 8, and an extension to cap 12) through the binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use beryl_cli::campaign::{StageStatus, StateEvent};
use beryl_cli::match_log::{GameRecord, files};

const BIN: &str = env!("CARGO_BIN_EXE_beryl_cli");

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("beryl-campaign-bin-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_plan(dir: &Path, name: &str, gate_challenger: &str) -> PathBuf {
    let plan = serde_json::json!({
        "name": name,
        "stages": [
            {
                "id": "gate-1",
                "kind": "gate",
                "challenger": gate_challenger,
                "champion": "pimc18/hidden/rollout/tuned/anchor",
                "seeds": 1,
                "work": "3k,10k"
            },
            {
                "id": "match-1",
                "kind": "match",
                "challenger": "greedy",
                "champion": "greedy+hold",
                "work": "3M",
                "bar": "stronger",
                "cap": 8,
                "seed": 5
            },
            {
                "id": "match-1-ext",
                "kind": "match",
                "extends": "match-1",
                "cap": 12
            }
        ]
    });
    let path = dir.join(format!("{name}.json"));
    fs::write(&path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    path
}

fn campaign_cmd(cache_dir: &Path, plan: &Path) -> Command {
    let mut c = Command::new(BIN);
    c.arg("campaign")
        .arg(plan)
        .env("BERYL_CACHE", cache_dir)
        .stdin(Stdio::null());
    c
}

fn strip_hours_line(report: &str) -> String {
    report
        .lines()
        .filter(|line| !line.starts_with("Hours:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "command failed ({})\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn finished_run_writes_report_with_match_row_and_gate_totals() {
    let root = scratch("finished");
    let cache = root.join("cache");
    let plan = write_plan(&root, "three-stage", "pimc18/hidden/rollout/tuned/anchor");
    let out = campaign_cmd(&cache, &plan)
        .args(["--commit", "deadbeef"])
        .output()
        .unwrap();
    assert_ok(&out);

    let report_path = cache
        .join("campaign")
        .join("three-stage")
        .join("report.txt");
    let report = fs::read_to_string(&report_path).unwrap();
    assert!(report.contains("Campaign: three-stage"), "{report}");
    assert!(report.contains("Commit: deadbeef"), "{report}");
    // Gate totals appear.
    assert!(
        report.contains("gate-1: champion") && report.contains("challenger"),
        "{report}"
    );
    // Match row for the extended 12-game match appears.
    assert!(
        report.contains("| greedy | greedy+hold | 3M | 12 |"),
        "{report}"
    );
    assert!(report.contains("games 12 (cap 12)"), "{report}");
    assert!(!report.contains("Failed:"), "{report}");
    assert!(!report.contains("Skipped:"), "{report}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn killed_runner_restarted_matches_unbroken_report() {
    let root = scratch("kill-resume");
    let plan = write_plan(&root, "resume-plan", "pimc18/hidden/rollout/tuned/anchor");

    let unbroken_cache = root.join("unbroken");
    let unbroken = campaign_cmd(&unbroken_cache, &plan)
        .args(["--commit", "abc1234"])
        .output()
        .unwrap();
    assert_ok(&unbroken);
    let unbroken_report = fs::read_to_string(
        unbroken_cache
            .join("campaign")
            .join("resume-plan")
            .join("report.txt"),
    )
    .unwrap();

    let killed_cache = root.join("killed");
    let mut child = campaign_cmd(&killed_cache, &plan)
        .args(["--commit", "abc1234"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let games_path = killed_cache
        .join("campaign")
        .join("resume-plan")
        .join("match-1")
        .join("games.jsonl");
    while files::read::<GameRecord>(&games_path)
        .unwrap_or_default()
        .is_empty()
        && child.try_wait().unwrap().is_none()
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = child.kill();
    let _ = child.wait();

    let resumed = campaign_cmd(&killed_cache, &plan)
        .args(["--commit", "abc1234"])
        .output()
        .unwrap();
    assert_ok(&resumed);
    let resumed_report = fs::read_to_string(
        killed_cache
            .join("campaign")
            .join("resume-plan")
            .join("report.txt"),
    )
    .unwrap();

    assert_eq!(
        strip_hours_line(&resumed_report),
        strip_hours_line(&unbroken_report)
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn crashing_stage_fails_after_three_tries_and_next_stage_runs() {
    let root = scratch("crash-stage");
    let cache = root.join("cache");
    let plan = write_plan(&root, "crash-plan", "pimc18/hidden/rollout/tuned/anchor");

    let out = campaign_cmd(&cache, &plan)
        .args(["--smoke", "--commit", "cafebabe"])
        .env("BERYL_CRASH_AT", "stage:gate-1")
        .output()
        .unwrap();
    assert_ok(&out);

    let dir = cache.join("campaign").join("crash-plan-smoke");
    let events: Vec<StateEvent> = files::read(&dir.join("state.jsonl")).unwrap();
    let gate_end = events
        .iter()
        .rev()
        .find(|e| e.stage == "gate-1")
        .expect("gate-1 event");
    assert_eq!(gate_end.status, StageStatus::Failed);
    assert!(
        gate_end
            .stderr
            .iter()
            .any(|l| l.contains("simulated crash in stage gate-1")),
        "{:?}",
        gate_end.stderr
    );

    let match_end = events
        .iter()
        .rev()
        .find(|e| e.stage == "match-1")
        .expect("match-1 event");
    assert_eq!(match_end.status, StageStatus::Finished);

    let report = fs::read_to_string(dir.join("report.txt")).unwrap();
    assert!(report.contains("Failed:"), "{report}");
    assert!(report.contains("gate-1:"), "{report}");
    assert!(report.contains("| greedy | greedy+hold |"), "{report}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn zero_hours_skips_every_stage_and_reports_why() {
    let root = scratch("zero-hours");
    let cache = root.join("cache");
    let plan = write_plan(&root, "zero-plan", "pimc18/hidden/rollout/tuned/anchor");

    let out = campaign_cmd(&cache, &plan)
        .args(["--hours", "0", "--commit", "0000000"])
        .output()
        .unwrap();
    assert_ok(&out);

    let dir = cache.join("campaign").join("zero-plan");
    let events: Vec<StateEvent> = files::read(&dir.join("state.jsonl")).unwrap();
    for id in ["gate-1", "match-1", "match-1-ext"] {
        let ev = events
            .iter()
            .rev()
            .find(|e| e.stage == id)
            .unwrap_or_else(|| panic!("missing event for {id}"));
        assert_eq!(ev.status, StageStatus::Skipped, "{id}");
    }

    let report = fs::read_to_string(dir.join("report.txt")).unwrap();
    assert!(report.contains("Skipped:"), "{report}");
    for id in ["gate-1", "match-1", "match-1-ext"] {
        assert!(report.contains(id), "{report}");
    }

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn unknown_option_fails_before_any_stage_starts() {
    let root = scratch("unknown-opt");
    let cache = root.join("cache");
    let plan = write_plan(
        &root,
        "bad-plan",
        "pimc18/hidden/rollout/tuned/anchor+nosuchopt",
    );

    let out = campaign_cmd(&cache, &plan).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nosuchopt"), "{stderr}");
    assert!(
        !cache.join("campaign").join("bad-plan").exists(),
        "campaign dir should not exist when validation fails"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn second_runner_on_same_directory_fails_and_names_first() {
    let root = scratch("lock-conflict");
    let cache = root.join("cache");
    let plan = write_plan(&root, "locked-plan", "pimc18/hidden/rollout/tuned/anchor");
    let dir = cache.join("campaign").join("locked-plan-smoke");
    fs::create_dir_all(&dir).unwrap();
    let held = files::lock(&dir).unwrap();

    let out = campaign_cmd(&cache, &plan).arg("--smoke").output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let pid = std::process::id().to_string();
    assert!(
        stderr.contains(&format!("process {pid}")),
        "expected stderr to name process {pid}, got:\n{stderr}"
    );

    drop(held);
    let _ = fs::remove_dir_all(&root);
}
