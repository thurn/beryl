//! The runner: plays the run's games one at a time, each search on every core, in the order
//! the schedule gives, logging each decision as it is made and committing each finished
//! game, so that whatever stops it costs at most the decision being searched.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::files::{self, Log};
use super::game::{self, Game, Line, Played};
use super::schedule::{self, Config, Matchup, Tally};
use super::{ClockRecord, DecisionRecord, GameRecord, Spec, Verdict, summary};
use crate::game::mirror;

/// The game and decision being played, for the panic hook.
static PLAYING: Mutex<Option<(PathBuf, String, usize)>> = Mutex::new(None);

/// A line of `crashes.jsonl`.
#[derive(Debug, Serialize, Deserialize)]
struct Crash {
    game: String,
    decision: usize,
    message: String,
}

/// A JSONL line, by the game it belongs to.
#[derive(Deserialize)]
struct Keyed {
    game: String,
}

/// Plays run `dir` until it holds `total` games, or forever.
pub fn run(dir: &Path, total: Option<u32>) -> Result<(), String> {
    let _lock = files::lock(dir)?;
    let c = super::config(dir)?;
    let t = mirror();
    let mut games = recover(dir)?;
    // A stop between a game's commit and the summary's rewrite leaves it behind.
    write_summary(dir, &c, &games)?;
    record_panics();
    // A fault injected for the supervisor's tests: panic at decision N of game K, "K@N".
    let inject: Option<(String, usize)> =
        std::env::var("BERYL_LONGTHINK_PANIC").ok().and_then(|v| {
            let (k, n) = v.rsplit_once('@')?;
            Some((k.to_owned(), n.parse().ok()?))
        });
    for sub in ["logs", "transcripts"] {
        fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    while total.is_none_or(|n| games.len() < n as usize) {
        let m = schedule::next(&c, |m| tally(&c, &games, m));
        let spec = Spec::new(&c, m.rung, m.against, played(&games, m));
        let path = dir.join("logs").join(format!("{}.jsonl", spec.key));
        files::repair(&path)?;
        let logged: Vec<Line> = files::read(&path)?;
        let record = if let Some(at) = crashed_twice(dir, &spec.key)? {
            crashed(&spec, &logged, at)
        } else {
            *PLAYING.lock().expect("the position") =
                Some((dir.to_owned(), spec.key.clone(), logged.len()));
            let mut log = Log::open(&path)?;
            let g = Game {
                seed: spec.seed,
                entrants: [&spec.entrants[0], &spec.entrants[1]],
                from: game::resume_points(&logged),
                until: None,
            };
            let asking = |i: usize| {
                if let Some((_, _, at)) = PLAYING.lock().expect("the position").as_mut() {
                    *at = i;
                }
                if inject
                    .as_ref()
                    .is_some_and(|(k, n)| *k == spec.key && *n == i)
                {
                    panic!("injected at decision {i} of {}", spec.key);
                }
            };
            // Only a search is worth a sync: a power cut costs the answers since the last
            // one, which replay recomputes at engine speed.
            let played = game::play(&t, &g, &game::build, &logged, asking, |_, line| {
                log.write(line, line.search.is_some())
            })
            .map_err(|e| format!("{}: {e:?}", spec.key))?;
            *PLAYING.lock().expect("the position") = None;
            commit(dir, &spec, &played)?
        };
        Log::open(&dir.join("games.jsonl"))?.append(&record)?;
        println!(
            "{}: {:?} on turn {}, {} rollouts",
            record.game, record.outcome, record.turns, record.rollouts
        );
        games.push(record);
        write_summary(dir, &c, &games)?;
    }
    Ok(())
}

/// The committed games, after cutting what an uncommitted game left in the other logs: one
/// game plays at a time and commits last, so its lines are each log's last.
fn recover(dir: &Path) -> Result<Vec<GameRecord>, String> {
    for name in [
        "games.jsonl",
        "decisions.jsonl",
        "clock.jsonl",
        "crashes.jsonl",
    ] {
        files::repair(&dir.join(name))?;
    }
    let games: Vec<GameRecord> = files::read(&dir.join("games.jsonl"))?;
    let committed: HashSet<&str> = games.iter().map(|g| g.game.as_str()).collect();
    for name in ["decisions.jsonl", "clock.jsonl"] {
        let path = dir.join(name);
        let lines: Vec<Keyed> = files::read(&path)?;
        let keep = lines
            .iter()
            .position(|l| !committed.contains(l.game.as_str()))
            .unwrap_or(lines.len());
        files::keep_lines(&path, keep)?;
    }
    Ok(games)
}

/// How many games matchup `m` has finished.
pub fn played(games: &[GameRecord], m: Matchup) -> u32 {
    games
        .iter()
        .filter(|g| (g.rung, g.against) == (m.rung, m.against))
        .count() as u32
}

/// What the schedule needs of matchup `m`'s finished games.
pub fn tally(c: &Config, games: &[GameRecord], m: Matchup) -> Tally {
    let mut t = Tally::default();
    for g in games
        .iter()
        .filter(|g| (g.rung, g.against) == (m.rung, m.against))
    {
        t.games += 1;
        if g.pair < c.pairs && g.outcome != Verdict::Crashed {
            t.first_scored += 1;
            t.first_score += match g.outcome {
                Verdict::Win => 1.0,
                Verdict::Draw | Verdict::Stalled => 0.5,
                Verdict::Loss | Verdict::Crashed => 0.0,
            };
        }
    }
    t
}

/// Records where the runner was when it panics, before the panic ends it.
fn record_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(Some((dir, game, decision))) = PLAYING.try_lock().as_deref() {
            let crash = Crash {
                game: game.clone(),
                decision: *decision,
                message: info.to_string(),
            };
            if let Ok(mut log) = Log::open(&dir.join("crashes.jsonl")) {
                let _ = log.append(&crash);
            }
        }
        default(info);
    }));
}

/// The decision game `key` has crashed at twice, if any.
fn crashed_twice(dir: &Path, key: &str) -> Result<Option<usize>, String> {
    let mut seen: HashMap<usize, u32> = HashMap::new();
    for crash in files::read::<Crash>(&dir.join("crashes.jsonl"))? {
        if crash.game == key {
            let n = seen.entry(crash.decision).or_default();
            *n += 1;
            if *n == 2 {
                return Ok(Some(crash.decision));
            }
        }
    }
    Ok(None)
}

fn crashed(spec: &Spec, logged: &[Line], at: usize) -> GameRecord {
    let searches = || logged.iter().filter_map(|l| l.search.as_ref());
    GameRecord {
        game: spec.key.clone(),
        rung: spec.rung,
        iterations: spec.entrants[usize::from(spec.seat)]
            .budget
            .map_or(0, |b| b.iterations),
        against: spec.against,
        pair: spec.pair,
        seed: spec.seed,
        seat: spec.seat,
        outcome: Verdict::Crashed,
        turns: 0,
        decisions: logged.len() as u32,
        searched: searches().count() as u32,
        rollouts: searches().map(|s| u64::from(s.rollouts)).sum(),
        crashed_at: Some(at),
    }
}

/// Writes a finished game's transcript, searched decisions and wall time, and returns the
/// record whose append to `games.jsonl` commits it.
fn commit(dir: &Path, spec: &Spec, played: &Played) -> Result<GameRecord, String> {
    files::write_whole(
        &dir.join("transcripts").join(format!("{}.txt", spec.key)),
        played.transcript.as_bytes(),
    )?;
    let mut decisions = Log::open(&dir.join("decisions.jsonl"))?;
    let mut searched = 0;
    let mut rollouts = 0;
    for (i, line) in played.lines.iter().enumerate() {
        let Some(s) = &line.search else { continue };
        let e = &spec.entrants[usize::from(line.seat)];
        searched += 1;
        rollouts += u64::from(s.rollouts);
        // One sync for the game's lines, before the line that commits it: each costs
        // milliseconds, as much as a cheap rung's search.
        decisions.write(
            &DecisionRecord {
                game: spec.key.clone(),
                decision: i,
                seat: line.seat,
                agent: e.name.clone(),
                iterations: e.budget.map_or(0, |b| b.iterations),
                choice: line.choice,
                agent_decision: s.decision,
                chain: s.chain,
                rollouts: s.rollouts,
                inherited: s.inherited,
                visits: s.visits.clone(),
                mean: s
                    .visits
                    .iter()
                    .zip(&s.rewards)
                    .map(|(&v, &r)| (v > 0).then(|| r / f64::from(v)))
                    .collect(),
            },
            false,
        )?;
    }
    decisions.sync()?;
    Log::open(&dir.join("clock.jsonl"))?.append(&ClockRecord {
        game: spec.key.clone(),
        seconds: played.lines.iter().map(|l| l.ms).sum::<u64>() as f64 / 1000.0,
    })?;
    let rung = &spec.entrants[usize::from(spec.seat)];
    Ok(GameRecord {
        game: spec.key.clone(),
        rung: spec.rung,
        iterations: rung.budget.map_or(0, |b| b.iterations),
        against: spec.against,
        pair: spec.pair,
        seed: spec.seed,
        seat: spec.seat,
        outcome: match played.outcome {
            beryl_engine::Outcome::Win(w) if w.index() == usize::from(spec.seat) => Verdict::Win,
            beryl_engine::Outcome::Win(_) => Verdict::Loss,
            beryl_engine::Outcome::Draw => Verdict::Draw,
            beryl_engine::Outcome::Stalled(_) => Verdict::Stalled,
        },
        turns: played.turns,
        decisions: played.lines.len() as u32,
        searched,
        rollouts,
        crashed_at: None,
    })
}

/// Rewrites summary.txt from the logs, and returns it.
pub fn write_summary(dir: &Path, c: &Config, games: &[GameRecord]) -> Result<String, String> {
    let seconds: HashMap<String, f64> = files::read::<ClockRecord>(&dir.join("clock.jsonl"))?
        .into_iter()
        .map(|r| (r.game, r.seconds))
        .collect();
    let name = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let text = summary::text(&name, c, games, &seconds);
    files::write_whole(&dir.join("summary.txt"), text.as_bytes())?;
    Ok(text)
}
