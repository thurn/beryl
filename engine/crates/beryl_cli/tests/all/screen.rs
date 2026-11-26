use beryl_cli::anchor_replay::games;
use beryl_engine::GameTable;
use beryl_search::Agent;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::TEST;
use beryl_cli::ladder::{Bar, Match, Runner, run};
use beryl_cli::screen::*;

const CHAMPION: &str = "pimc2/hidden/rollout/tuned@2k";

fn match_dir(name: &str) -> PathBuf {
    let out = std::env::temp_dir().join(format!("beryl-screen-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let m = Match {
        challenger: "greedy".to_owned(),
        champion: CHAMPION.to_owned(),
        work: 2_000,
        bar: Bar::Stronger,
        cap: 2,
        seed: 6,
        out: out.clone(),
        positions: Vec::new(),
        runner: Runner::Threads,
    };
    run(&TEST, &m, |_, _| {}).unwrap();
    out
}

/// A directory with a log that no longer fits its game, or one cut short mid-line, is
/// refused before any challenger is built, naming the game.
#[test]
fn a_log_that_no_longer_fits_is_refused_before_any_search() {
    let dir = match_dir("stale");
    let agents = AtomicUsize::new(0);
    let build = |t: &GameTable, seed, from| -> Result<Box<dyn Agent>, String> {
        agents.fetch_add(1, Ordering::Relaxed);
        builder(CHAMPION)?(t, seed, from)
    };
    let last = games(&dir).unwrap().pop().unwrap();
    let path = dir.join("logs").join(format!("{}.jsonl", last.game));
    let text = std::fs::read_to_string(&path).unwrap();
    // A choice past every candidate: the log of a game whose rules have since changed.
    let mut lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    lines[5]["choice"] = 999.into();
    let stale = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, stale).unwrap();
    let e = screen(&TEST, &dir, CHAMPION, &build, None).err().unwrap();
    assert!(
        e.contains(&last.game) && e.contains("no longer fits"),
        "{e}"
    );
    // A log cut short inside its last line, as by a crash mid-write.
    std::fs::write(&path, &text[..text.trim_end().len() - 3]).unwrap();
    let e = screen(&TEST, &dir, CHAMPION, &build, None).err().unwrap();
    assert!(e.contains(&format!("{}.jsonl", last.game)), "{e}");
    assert_eq!(agents.load(Ordering::Relaxed), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
