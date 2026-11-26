//! `beryl_cli ladder <challenger> [--vs NAME] [--work 3M] [--bar stronger|plausible|audit] [--cap N]
//! [--seed N] [--out DIR]` (`just ladder`): a paired match between a challenger and the
//! champion.
//!
//! - **The agents.** Each is a registered agent or a long-think entrant (`family@budget`); a
//!   family named without a budget plays at `--work`, the proxy budget of 3M engine steps a
//!   decision by default. The champion defaults to the registry's Deep Searcher family
//!   ([`beryl_search::DEEP_SEARCHER`]).
//! - **Paired seeds.** Pair `p` plays seed `game_seed(seed, 0, p)` twice, the challenger
//!   in the first seat and then in the second, [`games_at_once`] games at a time. A seed
//!   reproduces the match: the verdict is read off the pairs in order, so games still running
//!   when it comes are not counted.
//! - **A game is a process.** The command plays each game in a child process,
//!   `beryl_cli ladder-game` ([`game`]), since a thread cannot contain a crash: the `search`
//!   profile aborts on a panic, and a stack overflow kills the process. A game whose process
//!   dies is played again from its log, and one that dies twice at the same decision is
//!   recorded as crashed: its pair is passed over and the report lists it.
//!   The library's tests play on threads ([`Runner`]).
//! - **The test.** A sequential probability ratio test over the pairs, swept, split or lost
//!   ([`Sprt`]), between the two Elo hypotheses of the bar ([`Bar`]), stopping on a decision
//!   or at the cap. The audit plays the full cap in complete pairs and checks the score's
//!   Wilson 95% interval.
//! - **The record.** It prints the games, the pairs, the challenger's score, an Elo estimate
//!   with its 95% interval, the verdict and a ready `docs/ladder.md` row, and writes
//!   `match.json` beside the games. The games go to `--out`, by default
//!   `$BERYL_CACHE/ladder/<date>-<challenger>/`, laid out as a long-think run's
//!   ([`crate::match_log`]): `header.json` (the match's challenger, champion, budget, bar,
//!   seed and positions, [`Header`]), `games.jsonl`, `logs/<game>.jsonl` (each decision's
//!   answer, appended as it is made, which replays the game from its seed, as
//!   `anchor_replay::walk` does), `transcripts/<game>.txt`, `results/<game>.json` (the record
//!   a game's process writes when it finishes, for the match to read) and `decisions.jsonl`
//!   (each searched decision's root visits and means, written when its game ends).
//! - **Continuing.** Run again with the same `--out`, a match continues where it stopped, if
//!   its header agrees; `--cap` may be raised. The games in `games.jsonl` are counted again
//!   in seed order and never replayed, so the verdict is the one an unbroken match reaches,
//!   and a match already decided plays nothing. A game in flight when the run stopped
//!   replays through its log and is asked again only where the log ends
//!   ([`crate::match_log::game`]). If it does not replay through its log, the binary has
//!   changed since, and the game is played again from the start. A game whose process
//!   outlived the stopped run is waited for: it holds its lock until it exits.
//!
//! With `--positions DIR`, a set of curated start positions ([`crate::positions`]), four pairs
//! in five start from a position, both games from the same one with the libraries reshuffled
//! by the pair's seed, and the fifth from the shuffle, so mulligans and openings stay tested
//! (§4.1). The positions used are copied to the match's `positions/`, which each game's record
//! names, and the report sets the spread of pair scores from positions beside the shuffle's.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use beryl_search::{DEEP_SEARCHER, Family, Settings};

use crate::Catalog;
use crate::game::{mirror, parse_work};
use crate::match_log::files::{self, Log};
use crate::match_log::game::{self as arena, Entrant, GameDecision, Line};
use crate::match_log::{DecisionRecord, GameRecord, Verdict as GameVerdict, cache, rung_name};

pub mod game;
mod processes;
mod report;
mod sprt;

pub use report::{Header, Match, Report, Runner, dropped_row};
pub use sprt::{Bar, Pairs, Sprt, Verdict};

/// Games played at once, one a hardware thread:
/// 18 on an M5 Max, 20 on an i7-12700F, where 20 play 6% more games an hour than 18.
/// Games count in seed order, so how many run at once changes no result.
pub fn games_at_once() -> usize {
    thread::available_parallelism().map_or(1, usize::from)
}

/// The proxy budget, in engine steps a decision.
pub const PROXY: u64 = 3_000_000;

const USAGE: &str = "usage: beryl_cli ladder <challenger> [--vs NAME] [--work 3M] \
                     [--bar stronger|plausible|audit] [--cap N] [--seed N] [--positions DIR] \
                     [--out DIR]\n\
                     The same --out continues its match, whose --cap may be raised.";

pub fn main(catalog: &'static Catalog, args: &[String]) -> Result<(), String> {
    let mut challenger = None;
    let mut champion = None;
    let mut work = PROXY;
    let mut bar = Bar::Stronger;
    let mut cap = 2_000;
    let mut seed = 0;
    let mut out = None;
    let mut positions = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--vs" => champion = Some(value()?),
            "--work" => {
                let v = value()?;
                work = parse_work(&v).ok_or_else(|| format!("--work: no budget {v:?}"))?;
            }
            "--bar" => {
                let v = value()?;
                bar = Bar::named(&v).ok_or_else(|| format!("--bar: no bar {v:?}\n{USAGE}"))?;
            }
            "--cap" => cap = crate::number(arg, &value()?)?,
            "--seed" => seed = crate::number(arg, &value()?)?,
            "--out" => out = Some(PathBuf::from(value()?)),
            "--positions" => positions = crate::positions::load(Path::new(&value()?))?,
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            name if challenger.is_none() => challenger = Some(name.to_owned()),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let challenger = resolve(&challenger.ok_or(USAGE)?, work)?;
    let champion = resolve(&champion.unwrap_or_else(|| DEEP_SEARCHER.to_string()), work)?;
    if cap < 2 {
        return Err("--cap: a match plays at least one pair".to_owned());
    }
    let out = out.unwrap_or_else(|| {
        let name = challenger.replace(['/', '@'], "_");
        cache()
            .join("ladder")
            .join(format!("{}-{name}", beryl_session::corpus::today()))
    });
    let m = Match {
        challenger,
        champion,
        work,
        bar,
        cap,
        seed,
        out,
        positions,
        runner: Runner::Processes(
            std::env::current_exe().map_err(|e| format!("finding this binary: {e}"))?,
        ),
    };
    #[cfg(windows)]
    if let Err(error) = disable_execution_speed_throttling() {
        eprintln!("ladder: could not disable Windows execution speed throttling: {error}");
    }
    let report = run(catalog, &m, |pairs, sprt| {
        let line = format!(
            "pairs {}: {} swept, {} split, {} lost",
            pairs.total(),
            pairs.swept,
            pairs.split,
            pairs.lost
        );
        if let Some(sprt) = sprt {
            let [lo, hi] = sprt.bounds();
            eprintln!("{line}; LLR {:.2} in [{lo:.2}, {hi:.2}]", sprt.llr(pairs));
        } else {
            eprintln!("{line}");
        }
    })?;
    print!("{}", report.text());
    Ok(())
}

/// Windows can leave idle-priority searches on the efficiency cores. Set the power policy on
/// this process, since `just ladder` starts Cargo and Cargo launches the actual search binary,
/// and on each game's process, which does not inherit it.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(super) fn disable_execution_speed_throttling() -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
        ProcessPowerThrottling, SetProcessInformation,
    };

    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    };
    let size = u32::try_from(size_of_val(&state)).expect("Win32 structure size fits in a DWORD");
    // SAFETY: The current-process pseudo-handle is valid, and `state` has the documented
    // layout and size for ProcessPowerThrottling and lives for the duration of the call.
    let success = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            std::ptr::from_ref(&state).cast(),
            size,
        )
    };
    if success == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The entrant `name` names, as a long-think label: a registered agent as it is, a family
/// without a budget at `work`.
pub fn resolve(name: &str, work: u64) -> Result<String, String> {
    if beryl_search::find_agent(name)?.is_some() {
        return Ok(name.to_owned());
    }
    if name.contains('@') {
        entrant(name)?;
        return Ok(name.to_owned());
    }
    let family: Family = name.parse()?;
    Ok(format!("{family}@{}", rung_name(work)))
}

/// The arena's entrant for `name`, searching on one thread: games run one to a core.
fn entrant(name: &str) -> Result<Entrant, String> {
    if beryl_search::find_agent(name)?.is_some() {
        return Ok(Entrant::Fixed(name.to_owned()));
    }
    match name.rsplit_once('@') {
        Some((family, work)) => Ok(Entrant::Search(Settings {
            family: family.parse()?,
            work: parse_work(work).ok_or_else(|| format!("{name:?}: no budget {work:?}"))?,
            threads: 1,
        })),
        None => Err(format!(
            "no agent {name:?}: a registered agent, a family or family@budget"
        )),
    }
}

/// The game a line of `decisions.jsonl` belongs to, which is all [`Shared::open`] reads.
#[derive(serde::Deserialize)]
struct OfGame {
    game: String,
}

/// What a match has finished and counted, which its games share as they finish.
struct Shared {
    /// Each finished game's challenger half points, by game; none for a crashed game.
    halves: BTreeMap<u32, Option<u32>>,
    /// The crashed games, by name, and the decision each crashed at.
    crashed: BTreeMap<String, GameDecision>,
    /// The first game of the next pair to count or pass over.
    next: u32,
    /// The pairs counted, in order, all and by how they started, and the challenger's games.
    pairs: Pairs,
    from_positions: Pairs,
    from_shuffle: Pairs,
    counted: u32,
    wins: u32,
    losses: u32,
    draws: u32,
    verdict: Option<Verdict>,
    games: Log,
    decisions: Log,
}

impl Shared {
    /// What the match in `out` has finished: each game in `games.jsonl`, not yet counted.
    fn open(out: &Path) -> Result<Self, String> {
        let games = out.join("games.jsonl");
        let records = files::read::<GameRecord>(&games)?;
        // A run stopped between a game's decisions and its record leaves decisions of a game
        // with no record at the end of the file. The game replays to its end and writes them
        // again, so they are cut off here.
        let recorded: BTreeSet<&str> = records.iter().map(|r| r.game.as_str()).collect();
        let decisions = out.join("decisions.jsonl");
        files::cut_last(&decisions, |d: &OfGame| !recorded.contains(d.game.as_str()))?;
        let mut sh = Self {
            halves: BTreeMap::new(),
            crashed: BTreeMap::new(),
            next: 0,
            pairs: Pairs::default(),
            from_positions: Pairs::default(),
            from_shuffle: Pairs::default(),
            counted: 0,
            wins: 0,
            losses: 0,
            draws: 0,
            verdict: None,
            games: Log::open(&games)?,
            decisions: Log::open(&decisions)?,
        };
        for r in &records {
            sh.note(r);
        }
        Ok(sh)
    }

    /// Notes finished game `r` as one to count, or to pass over if it crashed.
    fn note(&mut self, r: &GameRecord) {
        let g = 2 * r.pair + u32::from(r.seat);
        self.halves.insert(g, halves(r.outcome));
        if let Some(at) = r.crashed_at {
            self.crashed.insert(r.game.clone(), at);
        }
    }

    /// Records finished game `record`, whose log is `lines`: appends its searched decisions
    /// and its record, and counts the pairs it completes. Returns whether any was counted.
    fn finish(
        &mut self,
        m: &Match,
        sprt: Option<Sprt>,
        record: &GameRecord,
        lines: &[Line],
    ) -> Result<bool, String> {
        for (i, line) in lines.iter().enumerate() {
            let Some(s) = &line.search else { continue };
            self.decisions
                .write(&decision(m, record, i, line, s), false)?;
        }
        self.decisions.sync()?;
        self.games.append(record)?;
        self.note(record);
        Ok(count_pairs(self, m, sprt))
    }
}

/// Reports the pairs counted and the optional sequential test.
type Progress<'a> = dyn FnMut(Pairs, Option<Sprt>) + Send + 'a;

/// Plays match `m`, reporting the pairs and the optional sequential test as pairs are counted,
/// and writes its games and `match.json` to `m.out`.
pub fn run(
    catalog: &'static Catalog,
    m: &Match,
    mut progress: impl FnMut(Pairs, Option<Sprt>) + Send,
) -> Result<Report, String> {
    if m.bar == Bar::Audit && !m.cap.is_multiple_of(2) {
        return Err("--cap: an audit needs an even number of games".to_owned());
    }
    let entrants = [entrant(&m.challenger)?, entrant(&m.champion)?];
    fs::create_dir_all(&m.out).map_err(|e| format!("{}: {e}", m.out.display()))?;
    let _lock = files::lock(&m.out)?;
    start_or_continue(m)?;
    let sprt = m.bar.sprt();
    let games = m.cap - m.cap % 2;
    let mut sh = Shared::open(&m.out)?;
    if count_pairs(&mut sh, m, sprt) {
        progress(sh.pairs, sprt);
    }
    if sh.next > games {
        return Err(format!(
            "--cap {}: {} already holds {} games counted or crashed",
            m.cap,
            m.out.display(),
            sh.next
        ));
    }
    let todo: Vec<u32> = (0..games).filter(|g| !sh.halves.contains_key(g)).collect();
    let sh = match &m.runner {
        Runner::Threads => on_threads(catalog, m, sprt, &entrants, &todo, sh, &mut progress)?,
        Runner::Processes(exe) => processes::run(exe, m, sprt, &todo, sh, &mut progress)?,
    };
    let report = report(m, sprt, &sh);
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
    files::write_whole(&m.out.join("match.json"), json.as_bytes())?;
    Ok(report)
}

/// Plays games `todo` of match `m` on threads of this process, [`games_at_once`] at a time,
/// until they are played or the test decides.
fn on_threads(
    catalog: &'static Catalog,
    m: &Match,
    sprt: Option<Sprt>,
    entrants: &[Entrant; 2],
    todo: &[u32],
    sh: Shared,
    progress: &mut Progress<'_>,
) -> Result<Shared, String> {
    let t = mirror(catalog);
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(sh.verdict.is_some());
    let failure = Mutex::new(None);
    let shared = Mutex::new(sh);
    let progress = Mutex::new(progress);
    thread::scope(|scope| {
        for _ in 0..games_at_once().min(todo.len()) {
            crate::workers::spawn(scope, || {
                while !stop.load(Ordering::Relaxed) {
                    let Some(&g) = todo.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    let played = game::play_one(&t, m, entrants, g, None);
                    let mut sh = shared.lock().unwrap();
                    match played.and_then(|(r, played)| sh.finish(m, sprt, &r, &played.lines)) {
                        Ok(new) => {
                            if new {
                                (progress.lock().unwrap())(sh.pairs, sprt);
                            }
                            if sh.verdict.is_some() {
                                stop.store(true, Ordering::Relaxed);
                            }
                        }
                        Err(e) => {
                            failure.lock().unwrap().get_or_insert(e);
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    match failure.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(shared.into_inner().unwrap()),
    }
}

/// Starts match `m` in `m.out`, writing its `header.json`, or checks that the match the
/// directory already holds is `m`.
fn start_or_continue(m: &Match) -> Result<(), String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", m.out.display());
    let path = m.out.join("header.json");
    let header = m.header();
    match fs::read_to_string(&path) {
        Ok(text) => {
            let held: Header = serde_json::from_str(&text).map_err(|e| at(&e))?;
            if let Some(field) = held.differs(&header) {
                return Err(at(&format!(
                    "holds a match whose {field}: pass another --out"
                )));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if m.out.join("games.jsonl").exists() {
                return Err(at(&"holds games but no header.json: pass another --out"));
            }
            for sub in ["logs", "transcripts"] {
                fs::create_dir_all(m.out.join(sub)).map_err(|e| at(&e))?;
            }
            if !m.positions.is_empty() {
                crate::positions::write(&m.out.join("positions"), &m.positions)?;
            }
            let json = serde_json::to_string_pretty(&header).map_err(|e| e.to_string())? + "\n";
            files::write_whole(&path, json.as_bytes())
        }
        Err(e) => Err(at(&e)),
    }
}

/// The challenger's half points from a game it won, lost or drew (a stall counts as a draw);
/// none from a game that crashed.
fn halves(outcome: GameVerdict) -> Option<u32> {
    match outcome {
        GameVerdict::Win => Some(2),
        GameVerdict::Loss => Some(0),
        GameVerdict::Draw | GameVerdict::Stalled => Some(1),
        GameVerdict::Crashed => None,
    }
}

/// The report of match `m` from what its workers counted.
fn report(m: &Match, sprt: Option<Sprt>, sh: &Shared) -> Report {
    let mut report = Report {
        date: beryl_session::corpus::today(),
        challenger: m.challenger.clone(),
        champion: m.champion.clone(),
        budget: rung_name(m.work),
        bar: m.bar.name(),
        sprt,
        seed: m.seed,
        cap: m.cap,
        games: sh.counted,
        pairs: sh.pairs,
        from_positions: sh.from_positions,
        from_shuffle: sh.from_shuffle,
        spread_positions: 0.0,
        spread_shuffle: 0.0,
        wins: sh.wins,
        losses: sh.losses,
        draws: sh.draws,
        score: 0.0,
        score_interval: [0.0, 1.0],
        elo: 0.0,
        elo_interval: [0.0, 0.0],
        llr: None,
        bounds: None,
        verdict: sh.verdict.unwrap_or(match m.bar {
            Bar::Audit => Verdict::Audit,
            Bar::Stronger | Bar::Plausible => Verdict::Undecided,
        }),
        crashed: sh.crashed.clone(),
        out: m.out.display().to_string(),
    };
    report.tally();
    report
}

/// Counts the pairs both of whose games are in, in order, until the test decides, passing
/// over a pair with a crashed game. Returns whether any was counted.
fn count_pairs(sh: &mut Shared, m: &Match, sprt: Option<Sprt>) -> bool {
    let mut any = false;
    while sh.verdict.is_none() {
        let first = sh.next;
        let (Some(&a), Some(&b)) = (sh.halves.get(&first), sh.halves.get(&(first + 1))) else {
            break;
        };
        sh.next += 2;
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        for h in [a, b] {
            match h {
                2 => sh.wins += 1,
                0 => sh.losses += 1,
                _ => sh.draws += 1,
            }
        }
        sh.pairs.add(a + b);
        if m.position(first / 2).is_some() {
            sh.from_positions.add(a + b);
        } else {
            sh.from_shuffle.add(a + b);
        }
        sh.counted += 2;
        sh.verdict = sprt.and_then(|test| test.decide(sh.pairs));
        any = true;
    }
    any
}

/// The `decisions.jsonl` line of searched decision `i` of `record`'s game.
fn decision(
    m: &Match,
    record: &GameRecord,
    i: usize,
    line: &Line,
    s: &arena::Searched,
) -> DecisionRecord {
    let challenger = line.seat == record.seat;
    let agent = if challenger {
        &m.challenger
    } else {
        &m.champion
    };
    DecisionRecord {
        game: record.game.clone(),
        decision: GameDecision(i),
        seat: line.seat,
        agent: agent.clone(),
        budget: entrant(agent).ok().and_then(|e| e.budget()).unwrap_or(0),
        choice: line.choice,
        agent_decision: s.decision,
        chain: s.chain,
        iterations: s.rollouts,
        work: s.work,
        inherited: s.inherited,
        visits: s.visits.clone(),
        mean: if s.means.len() == s.visits.len() {
            s.means.clone()
        } else {
            s.visits
                .iter()
                .zip(&s.rewards)
                .map(|(&v, &r)| (v > 0).then(|| r / f64::from(v)))
                .collect()
        },
        anchor: s.anchor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("beryl-ladder-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn audit_counts_all_pairs_and_checks_the_score_interval() {
        let out = scratch("audit");
        fs::create_dir_all(&out).unwrap();
        let m = Match {
            challenger: "greedy".to_owned(),
            champion: "random".to_owned(),
            work: PROXY,
            bar: Bar::Audit,
            cap: 400,
            seed: 0,
            out: out.clone(),
            positions: Vec::new(),
            runner: Runner::Threads,
        };
        let test = m.bar.sprt();
        for (sweeps, score, verdict) in [
            (100, "score 50.0% [45.1, 54.9]", "audit: passed"),
            (80, "score 40.0% [35.3, 44.9]", "audit: failed"),
        ] {
            let mut sh = Shared::open(&out).unwrap();
            // The opening sweep streak is enough to stop either sequential test.
            for pair in 0..m.cap / 2 {
                let halves = if pair < sweeps { 2 } else { 0 };
                sh.halves
                    .extend([(2 * pair, Some(halves)), (2 * pair + 1, Some(halves))]);
                assert!(count_pairs(&mut sh, &m, test));
                assert!(sh.verdict.is_none(), "after pair {pair}");
            }
            let r = report(&m, test, &sh);
            assert_eq!((r.games, r.verdict), (400, Verdict::Audit));
            assert!(r.text().contains(score), "{}", r.text());
            assert!(r.text().lines().any(|line| line == verdict), "{}", r.text());
            assert!(r.row().contains(" | audit | "));
        }
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn names_resolve_at_the_budget() {
        assert_eq!(
            resolve("pimc72/hidden/rollout/tuned/anchor", 3_000_000).unwrap(),
            "pimc72/hidden/rollout/tuned/anchor@3M"
        );
        assert_eq!(
            resolve(&DEEP_SEARCHER.to_string(), 3_000_000).unwrap(),
            "turnbudget72/hidden/rollout/tuned/anchor+guard+hold+waste+combat+sure+paired+walker@3M"
        );
        assert_eq!(
            resolve("pimc18/hidden/rollout/tuned@10M", 3_000_000).unwrap(),
            "pimc18/hidden/rollout/tuned@10M"
        );
        assert_eq!(resolve("greedy", 3_000_000).unwrap(), "greedy");
        assert!(resolve("nonsense", 3_000_000).is_err());
    }
}
