//! The long-think runner from outside, as `just longthink` runs it: tiny ladders of a few
//! games at 4 iterations a decision, stopped every way a run can be, must end with the logs an
//! uninterrupted run writes.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::Duration;

mod exe;

/// A ladder small enough for a test: rungs of 4 and 8 iterations, one tree, a pair each.
const TINY: [&str; 10] = [
    "--rungs",
    "4,8",
    "--cheap",
    "1",
    "--pairs",
    "1",
    "--trees",
    "1",
    "--opponent",
    "expert",
];

/// An empty directory for test `name`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl-longthink-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn longthink(cache: &Path, args: &[&str]) -> Output {
    Command::new(exe::beryl_cli())
        .arg("longthink")
        .args(args)
        .env("BERYL_CACHE", cache)
        .env_remove("BERYL_LONGTHINK_PANIC")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// Starts a tiny run in `dir`, playing nothing.
fn tiny(dir: &Path) {
    let mut args = vec!["--run", dir.to_str().unwrap(), "--games", "0"];
    args.extend(TINY);
    let o = longthink(dir, &args);
    assert!(o.status.success(), "{}", text(&o));
}

/// The run's own runner on `dir`, until it holds `games` games.
fn runner(dir: &Path, games: u32) -> Command {
    let mut c = Command::new(dir.join("beryl_cli"));
    c.arg("longthink-run")
        .arg(dir)
        .args(["--games", &games.to_string()])
        .stdout(Stdio::null())
        .env_remove("BERYL_LONGTHINK_PANIC");
    c
}

fn finish(dir: &Path, games: u32) {
    let o = runner(dir, games).stderr(Stdio::piped()).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
}

/// How much the run has written: grows with every decision.
fn progress(dir: &Path) -> u64 {
    let size = |p: PathBuf| fs::metadata(p).map_or(0, |m| m.len());
    let logs: u64 = fs::read_dir(dir.join("logs")).map_or(0, |d| {
        d.filter_map(Result::ok).map(|e| size(e.path())).sum()
    });
    logs + size(dir.join("games.jsonl"))
}

/// The files that must not depend on how the run was stopped, by name.
fn results(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![
        (
            "games.jsonl".to_owned(),
            fs::read(dir.join("games.jsonl")).unwrap(),
        ),
        (
            "decisions.jsonl".to_owned(),
            fs::read(dir.join("decisions.jsonl")).unwrap(),
        ),
    ];
    let mut transcripts: Vec<_> = fs::read_dir(dir.join("transcripts"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    transcripts.sort();
    for t in transcripts {
        let name = t.file_name().unwrap().to_string_lossy().into_owned();
        files.push((name, fs::read(t).unwrap()));
    }
    files
}

fn assert_same(a: &Path, b: &Path) {
    let (a, b) = (results(a), results(b));
    let names = |r: &[(String, Vec<u8>)]| r.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b));
    for ((name, x), (_, y)) in a.iter().zip(&b) {
        assert!(x == y, "{name} differs");
    }
}

fn kill(mut child: Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}

/// Killed at random moments, mid-search and mid-write among them, and resumed each time,
/// a run ends with the games, searched decisions and transcripts of an uninterrupted one.
/// Then a run whose logs end in torn lines, with its summary deleted, ends the same too.
#[test]
fn stops_cost_nothing_but_time() {
    let whole = scratch("whole");
    tiny(&whole);
    finish(&whole, 2);
    let games = fs::read_to_string(whole.join("games.jsonl")).unwrap();
    assert_eq!(games.lines().count(), 2, "{games}");

    let killed = scratch("killed");
    tiny(&killed);
    let mut rng = 0x2545_f491_4f6c_dd1du64;
    let mut kills = 0;
    loop {
        let mut child = runner(&killed, 2).spawn().unwrap();
        let start = progress(&killed);
        // Kill only once this runner has made progress of its own, so every resume moves on.
        let exited = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if progress(&killed) > start {
                break None;
            }
            thread::sleep(Duration::from_millis(1));
        };
        if let Some(status) = exited {
            assert!(status.success(), "{status}");
            break;
        }
        if kills == 40 {
            assert!(child.wait().unwrap().success());
            break;
        }
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        thread::sleep(Duration::from_micros(rng % 20_000));
        kill(child);
        kills += 1;
    }
    assert!(kills > 0);
    assert_same(&whole, &killed);

    // A torn last line in every log a stop can tear, and no summary.
    let torn = scratch("torn");
    tiny(&torn);
    finish(&torn, 1);
    let first = games.lines().next().unwrap();
    let second: serde_json::Value = serde_json::from_str(games.lines().nth(1).unwrap()).unwrap();
    let key = second["game"].as_str().unwrap();
    let log = fs::read_to_string(whole.join("logs").join(format!("{key}.jsonl"))).unwrap();
    let mut begun: String = log.split_inclusive('\n').take(10).collect();
    begun.push_str(&log.lines().nth(10).unwrap()[..7]);
    fs::write(torn.join("logs").join(format!("{key}.jsonl")), begun).unwrap();
    let append = |name: &str, tail: &str| {
        let mut text = fs::read_to_string(torn.join(name)).unwrap_or_default();
        text.push_str(tail);
        fs::write(torn.join(name), text).unwrap();
    };
    append("games.jsonl", &first[..20]);
    append("decisions.jsonl", "{\"game\":");
    append("clock.jsonl", "{");
    fs::remove_file(torn.join("summary.txt")).unwrap();
    finish(&torn, 2);
    assert_same(&whole, &torn);
    let summary = fs::read_to_string(torn.join("summary.txt")).unwrap();
    assert!(summary.contains("2 games"), "{summary}");
}

/// A second runner on a run already being played names the process playing it and leaves.
#[test]
fn one_runner_per_run() {
    let dir = scratch("locked");
    tiny(&dir);
    let first = runner(&dir, 1000).spawn().unwrap();
    let pid = first.id().to_string();
    while fs::read_to_string(dir.join("lock"))
        .unwrap_or_default()
        .trim()
        != pid
    {
        thread::sleep(Duration::from_millis(1));
    }
    let o = longthink(&dir, &["--run", dir.to_str().unwrap()]);
    kill(first);
    assert!(!o.status.success());
    assert!(text(&o).contains(&format!("process {pid}")), "{}", text(&o));
}

/// A runner that panics is restarted; a game that panics twice at one decision is recorded
/// as crashed, where, and skipped. `--status` then prints the ladder and plays nothing.
#[test]
fn a_game_that_crashes_twice_is_skipped() {
    let dir = scratch("crash");
    tiny(&dir);
    let o = Command::new(exe::beryl_cli())
        .args(["longthink", "--run", dir.to_str().unwrap(), "--games", "2"])
        .env("BERYL_LONGTHINK_PANIC", "4-v-expert-p000-s0@5")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(text(&o).matches("restarting it").count(), 2, "{}", text(&o));
    let games = fs::read_to_string(dir.join("games.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = games
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{games}");
    assert_eq!(lines[0]["outcome"], "crashed");
    assert_eq!(lines[0]["crashed_at"], 5);
    assert_ne!(lines[1]["outcome"], "crashed");

    let logs = fs::read_dir(dir.join("logs")).unwrap().count();
    let o = longthink(&dir, &["--run", dir.to_str().unwrap(), "--status"]);
    assert!(o.status.success(), "{}", text(&o));
    let status = String::from_utf8_lossy(&o.stdout);
    assert!(status.contains("2 games   1 crashed"), "{status}");
    assert!(
        status
            .lines()
            .any(|l| l.starts_with("4  ") && l.contains("2/2")),
        "{status}"
    );
    assert!(
        !status.contains('\x1b') && !status.contains("decision"),
        "{status}"
    );
    let summary = fs::read_to_string(dir.join("summary.txt")).unwrap();
    assert!(summary.contains("crashed: 4-v-expert-p000-s0"), "{summary}");
    assert_eq!(fs::read_to_string(dir.join("games.jsonl")).unwrap(), games);
    assert_eq!(fs::read_dir(dir.join("logs")).unwrap().count(), logs);
}

/// With no arguments the ladder starts in an empty cache and resumes when run again; it says
/// when the code differs from the run's, and `--new` starts another.
#[test]
fn no_arguments_start_or_resume_the_latest_run() {
    let cache = scratch("cache");
    let o = longthink(&cache, &["--status"]);
    assert!(!o.status.success(), "{}", text(&o));
    let run = cache.join("longthink/run-001");
    // `--games 0` plays nothing: the default ladder is far too dear for a test.
    let o = longthink(&cache, &["--games", "0"]);
    assert!(
        text(&o).contains(&format!("new run {}", run.display())),
        "{}",
        text(&o)
    );
    assert!(run.join("beryl_cli").exists() && run.join("config.json").exists());
    let o = longthink(&cache, &["--games", "0"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains(&format!("resuming {} (0 games)", run.display())),
        "{}",
        text(&o)
    );
    assert!(!text(&o).contains("differs"), "{}", text(&o));

    let code = run.join("code.json");
    let ids = fs::read_to_string(&code).unwrap();
    let mut ids: serde_json::Value = serde_json::from_str(&ids).unwrap();
    ids["beryl_search"] = "0000".into();
    fs::write(&code, ids.to_string()).unwrap();
    let o = longthink(&cache, &["--games", "0"]);
    assert!(
        text(&o).contains("the code differs from the run's in beryl_search:"),
        "{}",
        text(&o)
    );

    let o = longthink(&cache, &["--new", "--games", "0"]);
    assert!(
        text(&o).contains("new run") && text(&o).contains("run-002"),
        "{}",
        text(&o)
    );
    let o = longthink(&cache, &["--games", "0"]);
    assert!(text(&o).contains("run-002 (0 games)"), "{}", text(&o));
}

/// A `rust/target` cloned into another checkout runs the binary beside it, not the one the
/// test was compiled beside: a copy of this test binary, in a target of its own, spawns that
/// target's `beryl_cli`, here a script that says where it is.
#[test]
fn a_moved_target_runs_its_own_binary() {
    const MOVED: &str = "BERYL_MOVED_TARGET";
    if std::env::var_os(MOVED).is_some() {
        let o = Command::new(exe::beryl_cli()).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "moved\n");
        return;
    }
    let dir = scratch("moved");
    let deps = dir.join("target/debug/deps");
    fs::create_dir_all(&deps).unwrap();
    let me = std::env::current_exe().unwrap();
    let copy = deps.join(me.file_name().unwrap());
    fs::copy(&me, &copy).unwrap();
    let script = dir.join("target/debug/beryl_cli");
    fs::write(&script, "#!/bin/sh\necho moved\n").unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    let o = Command::new(&copy)
        .args([
            "--exact",
            "a_moved_target_runs_its_own_binary",
            "--nocapture",
        ])
        .env(MOVED, "1")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("1 passed"), "{}", text(&o));
    let _ = fs::remove_dir_all(&dir);
}
