//! `beryl_cli ladder <challenger> [--vs NAME] [--work 3M] [--bar stronger|plausible] [--cap N]
//! [--seed N] [--out DIR]` (`just ladder`): a paired match between a challenger and the
//! champion, ended by a sequential test (docs/ai-strategy.md §4).
//!
//! - **The agents.** Each is a registered agent or a long-think entrant (`family@budget`); a
//!   family named without a budget plays at `--work`, the proxy budget of 3M engine steps a
//!   decision by default. The champion defaults to the registry's Deep Searcher family
//!   ([`beryl_search::DEEP_SEARCHER`]).
//! - **Paired seeds.** Pair `p` plays seed [`game_seed`]`(seed, 0, p)` twice, the challenger
//!   in the first seat and then in the second, [`games_at_once`] games at a time on one thread
//!   each. A seed reproduces the match: the verdict is read off the pairs in order, so games
//!   still running when it comes are not counted.
//! - **The test.** A sequential probability ratio test over the pairs, swept, split or lost
//!   ([`Sprt`]), between the two Elo hypotheses of the bar ([`Bar`]), stopping on a decision
//!   or at the cap.
//! - **The record.** It prints the games, the pairs, the challenger's score, an Elo estimate
//!   with its 95% interval, the verdict and a ready `docs/ladder.md` row, and writes
//!   `match.json` beside the games. The games go to `--out`, by default
//!   `$BERYL_CACHE/ladder/<date>-<challenger>/`, laid out as a long-think run's
//!   ([`crate::longthink`]): `games.jsonl`, `logs/<game>.jsonl` (each decision's answer, which
//!   replays the game from its seed, as `anchor_replay::walk` does), `transcripts/<game>.txt`
//!   and `decisions.jsonl` (each searched decision's root visits and means).
//!
//! With `--positions DIR`, a set of curated start positions ([`crate::positions`]), four pairs
//! in five start from a position, both games from the same one with the libraries reshuffled
//! by the pair's seed, and the fifth from the shuffle, so mulligans and openings stay tested
//! (§4.1). The positions used are copied to the match's `positions/`, which each game's record
//! names, and the report sets the spread of pair scores from positions beside the shuffle's.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use beryl_engine::{GameTable, Outcome};
use beryl_search::{Agent, AgentDecision, Budget, DEEP_SEARCHER, Family, LongSearch, Settings};

use crate::game::{mirror, parse_work};
use crate::longthink::files::{self, Log};
use crate::longthink::game::{self as arena, Entrant, Game, GameDecision, Line, Resume};
use crate::longthink::{DecisionRecord, GameRecord, Verdict as GameVerdict, cache, rung_name};
use crate::positions::start;
use crate::tournament::game_seed;

mod report;
mod sprt;

pub use report::{Match, Report};
pub use sprt::{Bar, Pairs, Sprt, Verdict};

/// Games played at once, one thread each and one a hardware thread (docs/ai-strategy.md
/// §4.1): 18 on an M5 Max, 20 on an i7-12700F, where 20 play 6% more games an hour than 18.
/// Games count in seed order, so how many run at once changes no result.
pub fn games_at_once() -> usize {
    thread::available_parallelism().map_or(1, usize::from)
}

/// The proxy budget, in engine steps a decision.
const PROXY: u64 = 3_000_000;

const USAGE: &str = "usage: beryl_cli ladder <challenger> [--vs NAME] [--work 3M] \
                     [--bar stronger|plausible] [--cap N] [--seed N] [--positions DIR] \
                     [--out DIR]";

pub fn main(args: &[String]) -> Result<(), String> {
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
                bar = match value()?.as_str() {
                    "stronger" => Bar::Stronger,
                    "plausible" => Bar::Plausible,
                    other => return Err(format!("--bar: no bar {other:?}\n{USAGE}")),
                }
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
    };
    let report = run(&m, |pairs, llr, bounds| {
        eprintln!(
            "pairs {}: {} swept, {} split, {} lost; LLR {llr:.2} in [{:.2}, {:.2}]",
            pairs.total(),
            pairs.swept,
            pairs.split,
            pairs.lost,
            bounds[0],
            bounds[1]
        );
    })?;
    print!("{}", report.text());
    Ok(())
}

/// The entrant `name` names, as a long-think label: a registered agent as it is, a family
/// without a budget at `work`.
pub fn resolve(name: &str, work: u64) -> Result<String, String> {
    if name.contains('@') || beryl_search::agent(name).is_some() {
        entrant(name)?;
        return Ok(name.to_owned());
    }
    let family: Family = name.parse()?;
    Ok(format!("{family}@{}", rung_name(work)))
}

/// The arena's entrant for `name`, searching on one thread: games run one to a core.
fn entrant(name: &str) -> Result<Entrant, String> {
    match name.rsplit_once('@') {
        Some((family, work)) => Ok(Entrant::Search(Settings {
            family: family.parse()?,
            work: parse_work(work).ok_or_else(|| format!("{name:?}: no budget {work:?}"))?,
            threads: 1,
        })),
        None if beryl_search::agent(name).is_some() => Ok(Entrant::Fixed(name.to_owned())),
        None => Err(format!(
            "no agent {name:?}: a registered agent, a family or family@budget"
        )),
    }
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

/// Game `game` of a match: pair `game / 2`, the challenger in seat `game % 2`.
fn key(game: u32) -> String {
    format!("pair{:04}-seat{}", game / 2, game % 2)
}

/// What the workers share.
struct Shared {
    /// Each finished game's challenger half points, by game.
    halves: BTreeMap<u32, u32>,
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
    failure: Option<String>,
}

/// Plays match `m`, calling `progress` with the pairs, the LLR and the bounds each time a
/// pair is counted, and writes its games and `match.json` to `m.out`.
pub fn run(
    m: &Match,
    mut progress: impl FnMut(Pairs, f64, [f64; 2]) + Send,
) -> Result<Report, String> {
    if m.out.join("games.jsonl").exists() {
        return Err(format!(
            "{} already holds a match: pass another --out",
            m.out.display()
        ));
    }
    for sub in ["logs", "transcripts"] {
        fs::create_dir_all(m.out.join(sub)).map_err(|e| format!("{}: {e}", m.out.display()))?;
    }
    if !m.positions.is_empty() {
        crate::positions::write(&m.out.join("positions"), &m.positions)?;
    }
    let entrants = [entrant(&m.challenger)?, entrant(&m.champion)?];
    let sprt = m.bar.sprt();
    let t = mirror();
    let games = m.cap - m.cap % 2;
    let next = AtomicU32::new(0);
    let stop = AtomicBool::new(false);
    let shared = Mutex::new(Shared {
        halves: BTreeMap::new(),
        pairs: Pairs::default(),
        from_positions: Pairs::default(),
        from_shuffle: Pairs::default(),
        counted: 0,
        wins: 0,
        losses: 0,
        draws: 0,
        verdict: None,
        games: Log::open(&m.out.join("games.jsonl"))?,
        decisions: Log::open(&m.out.join("decisions.jsonl"))?,
        failure: None,
    });
    let progress = Mutex::new(&mut progress);
    let workers = games_at_once().min(games as usize);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= games {
                        break;
                    }
                    let played = play_one(&t, m, &entrants, g);
                    let mut sh = shared.lock().unwrap();
                    let counted = played.and_then(|(record, played)| {
                        record_game(&mut sh, m, &record, &played)?;
                        let halves = match record.outcome {
                            GameVerdict::Win => 2,
                            GameVerdict::Loss => 0,
                            _ => 1,
                        };
                        sh.halves.insert(g, halves);
                        Ok(count_pairs(&mut sh, m, sprt))
                    });
                    match counted {
                        Ok(new) => {
                            if new {
                                let pairs = sh.pairs;
                                (progress.lock().unwrap())(pairs, sprt.llr(pairs), sprt.bounds());
                            }
                            if sh.verdict.is_some() {
                                stop.store(true, Ordering::Relaxed);
                            }
                        }
                        Err(e) => {
                            sh.failure.get_or_insert(e);
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    let sh = shared.into_inner().unwrap();
    if let Some(e) = sh.failure {
        return Err(e);
    }
    let report = report(m, sprt, &sh);
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
    files::write_whole(&m.out.join("match.json"), json.as_bytes())?;
    Ok(report)
}

/// The report of match `m` from what its workers counted.
fn report(m: &Match, sprt: Sprt, sh: &Shared) -> Report {
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
        llr: 0.0,
        bounds: sprt.bounds(),
        verdict: sh.verdict.unwrap_or(Verdict::Undecided),
        out: m.out.display().to_string(),
    };
    report.tally();
    report
}

/// Counts the pairs both of whose games are in, in order, until the test decides. Returns
/// whether any was counted.
fn count_pairs(sh: &mut Shared, m: &Match, sprt: Sprt) -> bool {
    let mut any = false;
    while sh.verdict.is_none() {
        let first = sh.counted;
        let (Some(&a), Some(&b)) = (sh.halves.get(&first), sh.halves.get(&(first + 1))) else {
            break;
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
        sh.verdict = sprt.decide(sh.pairs);
        any = true;
    }
    any
}

/// Plays game `g` of match `m` and writes its log and transcript.
fn play_one(
    t: &GameTable,
    m: &Match,
    entrants: &[Entrant; 2],
    g: u32,
) -> Result<(GameRecord, arena::Played), String> {
    let seat = (g % 2) as usize;
    let seated = if seat == 0 {
        [&entrants[0], &entrants[1]]
    } else {
        [&entrants[1], &entrants[0]]
    };
    let seed = game_seed(m.seed, 0, g / 2);
    let position = m.position(g / 2);
    let game = Game {
        seed,
        entrants: seated,
        from: [Resume::At(AgentDecision(0)); 2],
        until: None,
        start: position.map(|(_, p)| start(t, p, seed)).transpose()?,
    };
    let key = key(g);
    let played = arena::play(t, &game, &build, &[], |_, _, _| Ok(()), |_, _, _| Ok(()))
        .map_err(|stop| format!("{key} (seed {seed}): {stop:?}"))?;
    let mut log = String::new();
    for line in &played.lines {
        log += &serde_json::to_string(line).map_err(|e| e.to_string())?;
        log.push('\n');
    }
    files::write_whole(
        &m.out.join("logs").join(format!("{key}.jsonl")),
        log.as_bytes(),
    )?;
    files::write_whole(
        &m.out.join("transcripts").join(format!("{key}.txt")),
        played.transcript.as_bytes(),
    )?;
    let (mut searched, mut iterations, mut work) = ([0; 2], [0; 2], [0; 2]);
    for l in &played.lines {
        // Sides are the challenger's then the champion's.
        let side = usize::from(usize::from(l.seat) != seat);
        if let Some(s) = &l.search {
            searched[side] += 1;
            iterations[side] += u64::from(s.rollouts);
            work[side] += s.work;
        }
    }
    let record = GameRecord {
        game: key,
        a: m.challenger.clone(),
        b: m.champion.clone(),
        pair: g / 2,
        seed,
        seat: seat as u8,
        outcome: match played.outcome {
            Outcome::Win(w) if w.index() == seat => GameVerdict::Win,
            Outcome::Win(_) => GameVerdict::Loss,
            Outcome::Draw => GameVerdict::Draw,
            Outcome::Stalled(_) => GameVerdict::Stalled,
        },
        turns: played.turns,
        decisions: played.lines.len() as u32,
        searched,
        iterations,
        work,
        crashed_at: None,
        position: position.map(|(name, _)| name.clone()),
    };
    Ok((record, played))
}

/// Appends a finished game's searched decisions and its record.
fn record_game(
    sh: &mut Shared,
    m: &Match,
    record: &GameRecord,
    played: &arena::Played,
) -> Result<(), String> {
    for (i, line) in played.lines.iter().enumerate() {
        let Some(s) = &line.search else { continue };
        sh.decisions
            .write(&decision(m, record, i, line, s), false)?;
    }
    sh.decisions.sync()?;
    sh.games.append(record)
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

    /// A pair's two games share their seed and swap the seats; every game leaves its log and
    /// transcript, and `match.json` the result.
    #[test]
    fn a_pair_shares_its_seed_and_swaps_seats() {
        let out = scratch("pairs");
        let m = Match {
            challenger: "greedy".to_owned(),
            champion: "random".to_owned(),
            work: PROXY,
            bar: Bar::Stronger,
            cap: 6,
            seed: 11,
            out: out.clone(),
            positions: Vec::new(),
        };
        let r = run(&m, |_, _, _| {}).unwrap();
        assert_eq!(
            (r.games, r.pairs.total(), r.verdict),
            (6, 3, Verdict::Undecided)
        );
        assert!(r.wins >= 4, "{r:?}");
        let games: Vec<GameRecord> = files::read(&out.join("games.jsonl")).unwrap();
        assert_eq!(games.len(), 6);
        for pair in 0..3 {
            let mut both: Vec<&GameRecord> = games.iter().filter(|g| g.pair == pair).collect();
            both.sort_by_key(|g| g.seat);
            assert_eq!(both.len(), 2);
            assert_eq!(both[0].seed, both[1].seed);
            assert_eq!(both[0].seed, game_seed(11, 0, pair));
            assert_eq!((both[0].seat, both[1].seat), (0, 1));
            for g in both {
                assert!(out.join("logs").join(format!("{}.jsonl", g.game)).exists());
                assert!(
                    out.join("transcripts")
                        .join(format!("{}.txt", g.game))
                        .exists()
                );
            }
        }
        assert!(out.join("match.json").exists());
        // The same seed plays the same games.
        let again = run(
            &Match {
                out: scratch("again"),
                ..m.clone()
            },
            |_, _, _| {},
        )
        .unwrap();
        assert_eq!(
            (again.wins, again.losses, again.pairs),
            (r.wins, r.losses, r.pairs)
        );
        assert!(
            run(&m, |_, _, _| {}).is_err(),
            "a second match in one directory"
        );
        let _ = fs::remove_dir_all(&out);
        let _ = fs::remove_dir_all(&again.out);
    }

    /// With positions, four pairs in five start from one, both games from the same, the set
    /// cycled through; the fifth starts from the shuffle. The games replay from their records.
    #[test]
    fn positions_start_four_pairs_in_five() {
        let set = crate::positions::build(
            &crate::positions::Build {
                games: 1,
                seed: 2,
                settings: Settings {
                    family: "pimc2/hidden/rollout/tuned".parse().unwrap(),
                    work: 2_000,
                    threads: 1,
                },
                band: [0.0, 1.0],
            },
            |_| {},
        )
        .unwrap()
        .positions;
        assert!(set.len() >= 2, "{}", set.len());
        let out = scratch("positions");
        let m = Match {
            challenger: "greedy".to_owned(),
            champion: "random".to_owned(),
            work: PROXY,
            bar: Bar::Stronger,
            cap: 12,
            seed: 4,
            out: out.clone(),
            positions: set.clone(),
        };
        let names: Vec<Option<&str>> = (0..6)
            .map(|p| m.position(p).map(|(n, _)| n.as_str()))
            .collect();
        let name = |k: usize| Some(set[k % set.len()].0.as_str());
        assert_eq!(names, [name(0), name(1), name(2), name(3), None, name(4)]);
        let r = run(&m, |_, _, _| {}).unwrap();
        assert_eq!((r.from_positions.total(), r.from_shuffle.total()), (5, 1));
        assert!(r.text().contains("from positions 5"));
        let games: Vec<GameRecord> = files::read(&out.join("games.jsonl")).unwrap();
        for g in &games {
            let expected = m.position(g.pair).map(|(n, _)| n.clone());
            assert_eq!(g.position, expected, "{}", g.game);
        }
        let mut visited = 0;
        let walked = crate::anchor_replay::walk(&out, "greedy", |_, _, _, _, _, _| visited += 1);
        assert_eq!(walked.unwrap(), 12);
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
            "pimc72/hidden/rollout/tuned/anchor@3M"
        );
        assert_eq!(
            resolve("pimc18/hidden/rollout/tuned@10M", 3_000_000).unwrap(),
            "pimc18/hidden/rollout/tuned@10M"
        );
        assert_eq!(resolve("greedy", 3_000_000).unwrap(), "greedy");
        assert!(resolve("nonsense", 3_000_000).is_err());
    }
}
