//! One game of a match, and `beryl_cli ladder-game --match DIR --game N`, the process the
//! `ladder` command plays each game in.
//!
//! A game is a process because a thread cannot contain its crash: the `search` profile aborts
//! on a panic, and a stack overflow kills the process whatever the profile. The command reads
//! the match from `DIR/header.json` (and its `positions/`), plays game `N` or continues it from
//! its log, as the match on threads does, and writes the game's [`GameRecord`] to
//! `DIR/results/<game>.json`. It exits [`FINISHED`] when the game is finished. While it plays it
//! holds the lock on `DIR/logs/<game>.lock`, and if another process holds that lock it exits
//! [`BUSY`] at once: a match whose own process was killed leaves its games playing on, and the
//! match run again must not play one twice at the same time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beryl_engine::{GameTable, Outcome};
use beryl_search::{Agent, AgentDecision, Budget, LongSearch};

use super::{Bar, Header, Match, Runner, entrant};
use crate::Catalog;
use crate::game::mirror;
use crate::match_log::files::{self, Log};
use crate::match_log::game::{self as arena, Entrant, Game, GameDecision, Line, Resume};
use crate::match_log::{GameRecord, Verdict as GameVerdict};
use crate::positions::start;
use crate::tournament::game_seed;

/// The exit code of a finished game, whose record is in `results/`.
pub const FINISHED: u8 = 0;
/// The exit code when another process holds the game's lock: the game was not started.
pub const BUSY: u8 = 75;
/// The exit code when the command could not read its arguments or the match: no game of the
/// match can be played, so the match stops.
pub const UNPLAYABLE: u8 = 2;

const USAGE: &str = "usage: beryl_cli ladder-game --match DIR --game N";

/// Runs `ladder-game` with `args`.
pub fn main(catalog: &'static Catalog, args: &[String]) -> ExitCode {
    let fail = |code: u8, e: &str| {
        eprintln!("beryl_cli ladder-game: {e}");
        ExitCode::from(code)
    };
    let (m, g) = match parse(args) {
        Ok(found) => found,
        Err(e) => return fail(UNPLAYABLE, &e),
    };
    let entrants = match entrant(&m.challenger).and_then(|a| Ok([a, entrant(&m.champion)?])) {
        Ok(entrants) => entrants,
        Err(e) => return fail(UNPLAYABLE, &e),
    };
    let key = key(g);
    let _lock = match files::lock_file(&lock_path(&m.out, g)) {
        Ok(Ok(lock)) => lock,
        Ok(Err(holder)) => {
            return fail(BUSY, &format!("{key} is being played by process {holder}"));
        }
        Err(e) => return fail(UNPLAYABLE, &e),
    };
    #[cfg(windows)]
    if let Err(error) = super::disable_execution_speed_throttling() {
        eprintln!("ladder-game: could not disable Windows execution speed throttling: {error}");
    }
    let abort_at = crash_at(&key);
    let t = mirror(catalog);
    let finished = crate::workers::run(|| {
        let (record, _) = play_one(&t, &m, &entrants, g, abort_at)?;
        let json = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())? + "\n";
        let results = m.out.join("results");
        fs::create_dir_all(&results).map_err(|e| format!("{}: {e}", results.display()))?;
        files::write_whole(&result_path(&m.out, g), json.as_bytes())
    });
    match finished {
        Ok(()) => ExitCode::from(FINISHED),
        // A game that fails is a crash: the match plays it again, then records it crashed.
        Err(e) => fail(1, &e),
    }
}

/// The match and game `args` name.
fn parse(args: &[String]) -> Result<(Match, u32), String> {
    let (mut dir, mut game) = (None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--match" => dir = Some(PathBuf::from(value()?)),
            "--game" => game = Some(crate::number(arg, &value()?)?),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let (Some(dir), Some(game)) = (dir, game) else {
        return Err(USAGE.to_owned());
    };
    Ok((open(&dir)?, game))
}

/// The match whose directory is `dir`, from its `header.json`. The cap is not kept there and a
/// game does not need it.
fn open(dir: &Path) -> Result<Match, String> {
    let path = dir.join("header.json");
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = fs::read_to_string(&path).map_err(|e| at(&e))?;
    let h: Header = serde_json::from_str(&text).map_err(|e| at(&e))?;
    let positions = if h.positions.is_empty() {
        Vec::new()
    } else {
        let all = crate::positions::load(&dir.join("positions"))?;
        h.positions
            .iter()
            .map(|name| {
                all.iter()
                    .find(|(n, _)| n == name)
                    .cloned()
                    .ok_or_else(|| at(&format!("names position {name:?}, not in positions/")))
            })
            .collect::<Result<_, _>>()?
    };
    Ok(Match {
        bar: Bar::named(&h.bar).ok_or_else(|| at(&format!("no bar {:?}", h.bar)))?,
        challenger: h.challenger,
        champion: h.champion,
        work: h.work,
        cap: 0,
        seed: h.seed,
        out: dir.to_owned(),
        positions,
        runner: Runner::Threads,
    })
}

/// The decision of game `key` to abort before asking, from `BERYL_CRASH_AT=<game>:<decision>`.
/// Only tests set it, to stand for a game that crashes the process at a decision.
fn crash_at(key: &str) -> Option<GameDecision> {
    let v = std::env::var("BERYL_CRASH_AT").ok()?;
    let (game, decision) = v.rsplit_once(':')?;
    if game != key {
        return None;
    }
    decision.parse().ok().map(GameDecision)
}

/// Game `game` of a match: pair `game / 2`, the challenger in seat `game % 2`.
pub(super) fn key(game: u32) -> String {
    format!("pair{:04}-seat{}", game / 2, game % 2)
}

/// Game `g`'s log in match directory `dir`.
pub(super) fn log_path(dir: &Path, g: u32) -> PathBuf {
    dir.join("logs").join(format!("{}.jsonl", key(g)))
}

/// The lock a process playing game `g` holds.
pub(super) fn lock_path(dir: &Path, g: u32) -> PathBuf {
    dir.join("logs").join(format!("{}.lock", key(g)))
}

/// Where the process playing game `g` writes its record.
pub(super) fn result_path(dir: &Path, g: u32) -> PathBuf {
    dir.join("results").join(format!("{}.json", key(g)))
}

/// Builds a seat's agent; a registered one searches on its own thread.
fn build(
    t: &GameTable,
    e: &Entrant,
    seed: u64,
    from: AgentDecision,
) -> Result<Box<dyn Agent>, String> {
    match e {
        Entrant::Search(settings) => Ok(Box::new(LongSearch::resumed(t, *settings, seed, from))),
        Entrant::Fixed(name) => {
            let info =
                beryl_search::agent(name).ok_or_else(|| format!("no agent is named {name:?}"))?;
            let budget = Budget {
                parallel: false,
                ..Budget::DEFAULT
            };
            Ok((info.new)(t, seed, budget))
        }
    }
}

/// Game `g`'s record with nothing played yet: who sat where, its seed and its position.
fn unplayed(m: &Match, g: u32, outcome: GameVerdict) -> GameRecord {
    GameRecord {
        game: key(g),
        a: m.challenger.clone(),
        b: m.champion.clone(),
        pair: g / 2,
        seed: game_seed(m.seed, 0, g / 2),
        seat: (g % 2) as u8,
        outcome,
        turns: 0,
        decisions: 0,
        searched: [0; 2],
        iterations: [0; 2],
        work: [0; 2],
        crashed_at: None,
        position: m.position(g / 2).map(|(name, _)| name.clone()),
    }
}

/// The record of game `g`, whose process died twice before asking decision `at`.
pub(super) fn crashed(m: &Match, g: u32, at: GameDecision) -> GameRecord {
    GameRecord {
        decisions: at.0 as u32,
        crashed_at: Some(at),
        ..unplayed(m, g, GameVerdict::Crashed)
    }
}

/// Plays game `g` of match `m`, or continues it from its log, appending each decision's line
/// to the log as it is made, and writes its transcript. With `abort_at`, the process aborts
/// before that decision is asked.
pub(super) fn play_one(
    t: &GameTable,
    m: &Match,
    entrants: &[Entrant; 2],
    g: u32,
    abort_at: Option<GameDecision>,
) -> Result<(GameRecord, arena::Played), String> {
    let seat = (g % 2) as usize;
    let seated = if seat == 0 {
        [&entrants[0], &entrants[1]]
    } else {
        [&entrants[1], &entrants[0]]
    };
    let seed = game_seed(m.seed, 0, g / 2);
    let start = m
        .position(g / 2)
        .map(|(_, p)| start(t, p, seed))
        .transpose()?;
    let key = key(g);
    let path = log_path(&m.out, g);
    let logged: Vec<Line> = files::read(&path)?;
    let mut log = Log::open(&path)?;
    let play = |logged: &[Line], log: &mut Log| {
        let game = Game {
            seed,
            entrants: seated,
            from: resume_points(logged),
            until: None,
            start,
        };
        arena::play(
            t,
            &game,
            &build,
            logged,
            |i, _, _| {
                if abort_at == Some(i) {
                    std::process::abort();
                }
                Ok(())
            },
            // A searched line is synced: it took seconds to make. Any other is cheaper to make
            // again than to sync, and the next sync covers it.
            |_, line, _| log.write(line, line.search.is_some()),
        )
    };
    let played = match play(&logged, &mut log) {
        // A log this binary does not replay to its end was written by another build.
        Err(stop) if !logged.is_empty() => {
            eprintln!("ladder: {key}: {stop}: the binary has changed, so it starts again");
            log.clear()?;
            play(&[], &mut log)
        }
        played => played,
    }
    .map_err(|stop| format!("{key} (seed {seed}): {stop}"))?;
    files::write_whole(
        &m.out.join("transcripts").join(format!("{key}.txt")),
        played.transcript.as_bytes(),
    )?;
    let mut record = unplayed(
        m,
        g,
        match played.outcome {
            Outcome::Win(w) if w.index() == seat => GameVerdict::Win,
            Outcome::Win(_) => GameVerdict::Loss,
            Outcome::Draw => GameVerdict::Draw,
            Outcome::Stalled(_) => GameVerdict::Stalled,
        },
    );
    record.turns = played.turns;
    record.decisions = played.lines.len() as u32;
    for l in &played.lines {
        // Sides are the challenger's then the champion's.
        let side = usize::from(usize::from(l.seat) != seat);
        if let Some(s) = &l.search {
            record.searched[side] += 1;
            record.iterations[side] += u64::from(s.rollouts);
            record.work[side] += s.work;
        }
    }
    Ok((record, played))
}

/// Where each seat's search starts again on a game continued through `logged`: the point its
/// last line there says a rebuilt agent must start from, or its first decision.
fn resume_points(logged: &[Line]) -> [Resume; 2] {
    [0, 1].map(|seat| {
        let last = logged.iter().rev().find(|l| l.seat == seat);
        Resume::At(last.and_then(|l| l.resume).unwrap_or(AgentDecision(0)))
    })
}
