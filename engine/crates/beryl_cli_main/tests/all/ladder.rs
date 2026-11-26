//! `beryl_cli ladder`, whose games are child processes: a match through them equals the same
//! match on threads, a game that crashes is skipped, and a match whose process is killed
//! continues when run again.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use beryl_cli::Catalog;
use beryl_cli::ladder::{Bar, Match, PROXY, Runner, run};
use beryl_cli::match_log::{GameRecord, Verdict, files};

/// The binary under test.
const BIN: &str = env!("CARGO_BIN_EXE_beryl_cli");

/// The catalog the binary plays with, for the same match on threads.
static CATALOG: Catalog = Catalog {
    cards: &beryl_cards::CARDS,
    scenarios: beryl_cards::scenarios,
    library_scenarios: || beryl_cards::library::by_card().collect(),
    derived_scenarios: |def| beryl_cards::derive::for_card(def).scenarios,
    fuzz_deck_from: beryl_cards::decks::fuzz_deck_from,
    keystone_family: beryl_cards::decks::keystone_family,
};

const SEED: u64 = 5;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl-ladder-bin-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// `beryl_cli ladder greedy --vs greedy+hold` to cap 8 in `out`.
fn ladder(out: &Path) -> Command {
    let mut c = Command::new(BIN);
    c.args([
        "ladder",
        "greedy",
        "--vs",
        "greedy+hold",
        "--cap",
        "8",
        "--seed",
    ])
    .arg(SEED.to_string())
    .arg("--out")
    .arg(out)
    .stdin(Stdio::null())
    .stdout(Stdio::null());
    c
}

/// The same match played on threads of this process, in `out`.
fn on_threads(out: PathBuf) -> PathBuf {
    let m = Match {
        challenger: "greedy".to_owned(),
        champion: "greedy+hold".to_owned(),
        work: PROXY,
        bar: Bar::Stronger,
        cap: 8,
        seed: SEED,
        out: out.clone(),
        positions: Vec::new(),
        runner: Runner::Threads,
    };
    run(&CATALOG, &m, |_, _| {}).unwrap();
    out
}

/// `match.json` in `out`, without what names the directory or the day.
fn result_in(out: &Path) -> serde_json::Value {
    let text = fs::read_to_string(out.join("match.json")).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let fields = json.as_object_mut().unwrap();
    fields.remove("out");
    fields.remove("date");
    json
}

fn games_in(out: &Path) -> Vec<GameRecord> {
    let mut games: Vec<GameRecord> = files::read(&out.join("games.jsonl")).unwrap();
    games.sort_by(|a, b| a.game.cmp(&b.game));
    games
}

#[test]
fn a_match_on_processes_equals_the_match_on_threads() {
    let out = scratch("processes");
    assert!(ladder(&out).status().unwrap().success());
    let threads = on_threads(scratch("threads"));
    assert_eq!(result_in(&out), result_in(&threads));
    assert_eq!(games_in(&out), games_in(&threads));
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&threads);
}

#[test]
fn a_game_that_aborts_twice_at_a_decision_is_skipped() {
    let out = scratch("crash");
    let status = ladder(&out)
        .env("BERYL_CRASH_AT", "pair0001-seat1:5")
        .status()
        .unwrap();
    assert!(status.success());
    let games = games_in(&out);
    let crashed: Vec<&GameRecord> = games
        .iter()
        .filter(|g| g.outcome == Verdict::Crashed)
        .collect();
    assert_eq!(crashed.len(), 1, "{games:?}");
    assert_eq!(crashed[0].game, "pair0001-seat1");
    assert_eq!(crashed[0].crashed_at.map(|d| d.0), Some(5));
    let result = result_in(&out);
    assert_eq!(
        result["crashed"],
        serde_json::json!({ "pair0001-seat1": 5 })
    );
    assert_eq!(result["games"], 6);
    // The other three pairs are counted as the unbroken match counts them.
    let threads = on_threads(scratch("crash-threads"));
    let whole = games_in(&threads);
    let pair = |games: &[GameRecord], p: u32| -> Vec<Verdict> {
        games
            .iter()
            .filter(|g| g.pair == p)
            .map(|g| g.outcome)
            .collect()
    };
    let (mut wins, mut losses) = (0, 0);
    for p in [0, 2, 3] {
        assert_eq!(pair(&games, p), pair(&whole, p), "pair {p}");
        for v in pair(&whole, p) {
            wins += u32::from(v == Verdict::Win);
            losses += u32::from(v == Verdict::Loss);
        }
    }
    assert_eq!(
        (result["wins"].as_u64(), result["losses"].as_u64()),
        (Some(wins.into()), Some(losses.into()))
    );
    let pairs = &result["pairs"];
    let total = ["swept", "split", "lost"]
        .map(|k| pairs[k].as_u64().unwrap())
        .iter()
        .sum::<u64>();
    assert_eq!(total, 3);
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&threads);
}

#[test]
fn a_killed_match_run_again_equals_an_unbroken_one() {
    let out = scratch("killed");
    let mut first = ladder(&out).spawn().unwrap();
    // Wait for two recorded games, or for the match to end first on a fast machine.
    while files::read::<GameRecord>(&out.join("games.jsonl"))
        .unwrap_or_default()
        .len()
        < 2
        && first.try_wait().unwrap().is_none()
    {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Its games play on as orphans, which the match run again waits for.
    let _ = first.kill();
    first.wait().unwrap();
    assert!(ladder(&out).status().unwrap().success());
    let threads = on_threads(scratch("killed-threads"));
    assert_eq!(result_in(&out), result_in(&threads));
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&threads);
}
