//! The runner: plays the run's games one at a time, each search on every core, in the order
//! the schedule gives, logging each decision as it is made and committing each finished
//! game, so that whatever stops it costs at most the decision being searched.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use beryl_engine::{Decision, State};
use serde::{Deserialize, Serialize};

use super::files::{self, Log};
use super::game::{self, Game, Line, Played};
use super::schedule::{self, Config};
use super::standings::Standings;
use super::term::Display;
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

/// Plays run `dir` until it holds `total` games, or forever, or a stop is asked for.
pub fn run(dir: &Path, total: Option<u32>) -> Result<(), String> {
    let _lock = files::lock(dir)?;
    let c = super::config(dir)?;
    let t = mirror();
    let mut games = recover(dir)?;
    // A stop between a game's commit and the summary's rewrite leaves it behind.
    write_summary(dir, &c, &games)?;
    record_panics();
    let clock = clock(dir)?;
    let mut standings = Standings::new(&c, &games, &clock);
    let out = Display::start(&run_name(dir), &c, &games, clock)?;
    // A fault injected for the supervisor's tests: panic at decision N of game K, "K@N".
    let inject: Option<(String, usize)> =
        std::env::var("BERYL_LONGTHINK_PANIC").ok().and_then(|v| {
            let (k, n) = v.rsplit_once('@')?;
            Some((k.to_owned(), n.parse().ok()?))
        });
    for sub in ["logs", "transcripts"] {
        fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    while total.is_none_or(|n| games.len() < n as usize) && !out.stopping() {
        let m = schedule::next(&c, &standings);
        let spec = Spec::new(&c, m, standings.get(m).games);
        let path = dir.join("logs").join(format!("{}.jsonl", spec.key));
        files::repair(&path)?;
        let logged: Vec<Line> = files::read(&path)?;
        let (record, seconds) = if let Some(at) = crashed_twice(dir, &spec.key)? {
            (crashed(&spec, &logged, at), None)
        } else {
            out.begin(&spec, logged.len());
            *PLAYING.lock().expect("the position") =
                Some((dir.to_owned(), spec.key.clone(), logged.len()));
            let mut log = Log::open(&path)?;
            let g = Game {
                seed: spec.seed,
                entrants: [&spec.entrants[0], &spec.entrants[1]],
                from: game::resume_points(&logged),
                until: None,
            };
            let asking = |i: usize, s: &State, d: &Decision| {
                out.asking(i, s, d);
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
            let played = game::play(&t, &g, &game::build, &logged, asking, |_, line, label| {
                log.write(line, line.search.is_some())?;
                out.answered(line, label);
                Ok(())
            })
            .map_err(|e| format!("{}: {e:?}", spec.key))?;
            *PLAYING.lock().expect("the position") = None;
            (commit(dir, &spec, &played)?, Some(seconds(&played)))
        };
        Log::open(&dir.join("games.jsonl"))?.append(&record)?;
        out.finished(&record, seconds);
        standings.record(&c, &record, seconds);
        games.push(record);
        write_summary(dir, &c, &games)?;
    }
    out.close();
    Ok(())
}

/// Run `dir`'s name: its directory's.
pub fn run_name(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Each committed game's wall seconds.
pub fn clock(dir: &Path) -> Result<HashMap<String, f64>, String> {
    Ok(files::read::<ClockRecord>(&dir.join("clock.jsonl"))?
        .into_iter()
        .map(|r| (r.game, r.seconds))
        .collect())
}

/// A game's wall seconds: the time its decisions took.
fn seconds(played: &Played) -> f64 {
    played.lines.iter().map(|l| l.ms).sum::<u64>() as f64 / 1000.0
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
        super::term::restore();
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
        matchup: spec.matchup,
        iterations: spec.entrants[usize::from(spec.seat)]
            .budget
            .map_or(0, |b| b.iterations),
        pair: spec.pair,
        seed: spec.seed,
        seat: spec.seat,
        outcome: Verdict::Crashed,
        turns: 0,
        decisions: logged.len() as u32,
        searched: searches().count() as u32,
        rollouts: searches().map(|s| u64::from(s.rollouts)).sum(),
        budget: budget(spec, logged),
        crashed_at: Some(at),
    }
}

/// What the budgets of the searched decisions of `lines` allowed.
fn budget(spec: &Spec, lines: &[Line]) -> u64 {
    lines
        .iter()
        .filter(|l| l.search.is_some())
        .filter_map(|l| spec.entrants[usize::from(l.seat)].budget)
        .map(|b| u64::from(b.iterations))
        .sum()
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
        seconds: seconds(played),
    })?;
    let rung = &spec.entrants[usize::from(spec.seat)];
    Ok(GameRecord {
        game: spec.key.clone(),
        matchup: spec.matchup,
        iterations: rung.budget.map_or(0, |b| b.iterations),
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
        budget: budget(spec, &played.lines),
        crashed_at: None,
    })
}

/// Rewrites summary.txt from the logs, and returns it.
pub fn write_summary(dir: &Path, c: &Config, games: &[GameRecord]) -> Result<String, String> {
    let text = summary::text(&run_name(dir), c, games, &clock(dir)?);
    files::write_whole(&dir.join("summary.txt"), text.as_bytes())?;
    Ok(text)
}
