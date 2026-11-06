//! The long-think panel at a terminal, as `just longthink` shows it: tiny arenas in a
//! pseudo-terminal (`pty.py`, Python's standard `pty`), stopped by Ctrl-C once and twice;
//! and its output piped, with no terminal at all.

mod ansi;
mod exe;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ansi::Term;

const COLS: usize = 100;

/// This crate's directory, as cargo sets it when it runs the test: `env!` would bake in the
/// path of the checkout the test was compiled in (see `exe`).
fn manifest() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
}

/// An empty directory for test `name`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl-tty-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `longthink` on run `dir`, configured by `config` if it is new, piped.
fn piped(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(exe::beryl_cli())
        .arg("longthink")
        .args(["--run", dir.to_str().unwrap()])
        .args(args)
        .env("BERYL_CACHE", dir)
        .env_remove("BERYL_LONGTHINK_PANIC")
        .output()
        .unwrap()
}

fn games(dir: &Path) -> usize {
    fs::read_to_string(dir.join("games.jsonl")).map_or(0, |g| g.lines().count())
}

/// What a run at a terminal showed, how it ended, and the games before and after each press.
struct Pressed {
    bytes: Vec<u8>,
    code: i32,
    presses: Vec<[usize; 2]>,
}

/// `longthink` on run `dir` in a terminal, pressing Ctrl-C `presses` times, with `env` set.
fn at_a_terminal(dir: &Path, presses: u32, args: &[&str], env: &[(&str, &str)]) -> Pressed {
    let out = dir.join("tty.out");
    let o = Command::new("python3")
        .arg("-I")
        .arg(manifest().join("tests/pty.py"))
        .arg(&out)
        .args(["40", &COLS.to_string(), &presses.to_string()])
        .arg(dir.join("games.jsonl"))
        .arg("--")
        .arg(exe::beryl_cli())
        .args(["longthink", "--run", dir.to_str().unwrap()])
        .args(args)
        .env("BERYL_CACHE", dir)
        .env("TERM", "xterm-256color")
        .env_remove("NO_COLOR")
        .env_remove("BERYL_LONGTHINK_PANIC")
        .envs(env.iter().copied())
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    assert!(o.status.success(), "pty.py failed");
    let report: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    Pressed {
        bytes: fs::read(&out).unwrap(),
        code: i32::try_from(report["code"].as_i64().unwrap()).unwrap(),
        presses: report["presses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| [0, 1].map(|i| usize::try_from(p[i].as_u64().unwrap()).unwrap()))
            .collect(),
    }
}

/// What every stop at a terminal leaves: the cursor shown last, and one panel on screen,
/// the last frame, with the stop asked for, below a log line per game played.
fn assert_clean_exit(p: &Pressed, played: usize) {
    let text = String::from_utf8_lossy(&p.bytes);
    assert!(text.ends_with("\x1b[?25h"), "{text:?}");
    assert!(text.contains("\x1b[?25l"));
    let screen = Term::replay(&p.bytes, COLS).lines();
    let headers: Vec<&String> = screen
        .iter()
        .filter(|l| l.starts_with("longthink "))
        .collect();
    assert_eq!(headers.len(), 1, "{screen:#?}");
    assert!(headers[0].contains('⏸'), "{screen:#?}");
    let logged = screen
        .iter()
        .filter(|l| l.starts_with(['✓', '✗', '½']))
        .count();
    assert_eq!(logged, played, "{screen:#?}");
}

const TINY: [&str; 12] = [
    "--power",
    "light",
    "--families",
    "ismcts1/hidden/rollout/tuned",
    "--fixed",
    "expert",
    "--anchor",
    "expert",
    "--budgets",
    "1500",
    "--threads",
    "1",
];

/// The tiny arena's one search, and the key of its first game.
const SEARCH: &str = "ismcts1/hidden/rollout/tuned@1500";
const FIRST: &str = "expert~ismcts1.hidden.rollout.tuned@1500-p0000-s0";

/// One Ctrl-C finishes the game being played, then exits, leaving a run that goes on.
#[test]
fn one_ctrl_c_stops_after_the_game() {
    let dir = scratch("once");
    let mut args = vec!["--games", "1000"];
    args.extend(TINY);
    let p = at_a_terminal(&dir, 1, &args, &[]);
    assert_eq!(p.code, 0);
    let [[before, after]] = p.presses[..] else {
        panic!("{:?}", p.presses)
    };
    let n = games(&dir);
    assert!(before < n && n <= after + 1, "{before} {after} {n}");
    assert_clean_exit(&p, n);

    let o = piped(&dir, &["--games", &(n + 1).to_string()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(games(&dir), n + 1);
}

/// A second Ctrl-C stops at once, losing the game in progress, which the next run resumes.
#[test]
fn two_ctrl_cs_stop_at_once() {
    let dir = scratch("twice");
    let mut args = vec!["--games", "1000"];
    args.extend(TINY);
    // Games long enough that the second press comes mid-game: one that ends before the
    // second press is taken stops the run after it, and the test presses again, in the same
    // tiny arena. (Without it, the default arena's searches can outlast pty.py's deadline on
    // a busy machine.)
    let budget = args.iter().position(|&a| a == "--budgets").unwrap() + 1;
    args[budget] = "18k";
    let mut tries = 0;
    let (p, start) = loop {
        let start = games(&dir);
        let p = at_a_terminal(&dir, 2, &args, &[]);
        tries += 1;
        if p.code != 0 || tries == 5 {
            break (p, start);
        }
    };
    assert_eq!(p.code, 130);
    let [_, [before, after]] = p.presses[..] else {
        panic!("{:?}", p.presses)
    };
    assert_eq!((before, after), (games(&dir), games(&dir)));
    assert_clean_exit(&p, games(&dir) - start);
    // The next runner resumes the game from its last logged decision. A press before the
    // game's first leaves nothing to resume: the game starts over, as it should.
    if let Some(line) = first_game_line(&dir) {
        assert!(!line.ends_with("decision 0"), "{line}");
    }
}

/// The resume line a runner of the run in `dir` begins its first game with, if it resumes
/// one: read up to its first line but those of entrants opening, so at most one game.
fn first_game_line(dir: &Path) -> Option<String> {
    let mut runner = Command::new(dir.join("beryl_cli"))
        .arg("longthink-run")
        .arg(dir)
        .stdout(Stdio::piped())
        .env_remove("BERYL_LONGTHINK_PANIC")
        .spawn()
        .unwrap();
    let first = BufReader::new(runner.stdout.take().unwrap())
        .lines()
        .map(Result::unwrap)
        .find(|l| !l.starts_with('▸'));
    runner.kill().unwrap();
    runner.wait().unwrap();
    first.filter(|l| l.starts_with("↻ resumed"))
}

/// A runner that crashes after one Ctrl-C is not restarted to play on: the stop was asked
/// for, and the game resumes next time.
#[test]
fn a_crash_after_one_ctrl_c_still_stops() {
    let dir = scratch("crash");
    let mut args = vec!["--games", "4"];
    args.extend(TINY);
    // The first game's decision 100 of 125 comes most of the way through it.
    let at = format!("{FIRST}@100");
    let panic = [("BERYL_LONGTHINK_PANIC", at.as_str())];
    let p = at_a_terminal(&dir, 1, &args, &panic);
    assert_eq!(p.code, 0);
    assert!(
        String::from_utf8_lossy(&p.bytes).contains("after a stop was asked for"),
        "{}",
        String::from_utf8_lossy(&p.bytes)
    );
    assert_eq!(games(&dir), 0);
}

/// Piped, the runner writes no escapes: a line per game and a board every 10 games.
#[test]
fn piped_output_is_plain() {
    let dir = scratch("piped");
    let mut args = vec!["--games", "0"];
    args.extend(TINY);
    let o = piped(&dir, &args);
    assert!(o.status.success());
    // Nine games already played, so the next makes ten.
    let mut nine = String::new();
    for g in 0..9u32 {
        nine.push_str(
            &serde_json::json!({
                "game": format!("expert~ismcts1.hidden.rollout.tuned@1500-p{:04}-s{}", g / 2, g % 2),
                "a": "expert", "b": SEARCH, "pair": g / 2, "seed": 1, "seat": g % 2,
                "outcome": "win", "turns": 10, "decisions": 100, "searched": [0, 40],
                "iterations": [0, 160], "work": [0, 60_000],
            })
            .to_string(),
        );
        nine.push('\n');
    }
    fs::write(dir.join("games.jsonl"), nine).unwrap();
    let o = piped(&dir, &["--games", "10"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(!out.contains('\x1b'), "{out:?}");
    let lines: Vec<&str> = out.lines().collect();
    let game = lines
        .iter()
        .position(|l| l.contains(&format!("expert v {SEARCH}  s4·B")))
        .expect(&out);
    let table = lines
        .iter()
        .position(|l| l.starts_with("longthink "))
        .expect(&out);
    assert!(table > game, "{out}");
    assert!(
        lines[table..]
            .iter()
            .any(|l| l.starts_with(SEARCH) && l.split_whitespace().any(|c| c == "10")),
        "{out}"
    );
}
