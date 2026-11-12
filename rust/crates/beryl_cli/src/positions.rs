//! The ladder's start positions (docs/ai-strategy.md §4.1): balanced mid-game boards that take
//! the openings' luck out of a match's variance.
//!
//! - `beryl_cli positions build [--games N] [--seed N] [--work 3M] [--out DIR]` plays N seeded
//!   games of the champion ([`DEEP_SEARCHER`]) against itself at `--work` and cuts each as
//!   turns 3 to 8 begin, before the draw: the board as a save's turn-start checkpoint keeps it
//!   ([`Checkpoint`]). A cut is kept only where nothing lasts past the turn's beginning off the
//!   board ([`clean`]), and only where the
//!   champion's own estimate from each seat, a search at `--work`, lies within 0.45 to 0.55 for
//!   the seat to move. Each kept position is a file `<game>-t<turn>.json` under `--out`, by
//!   default `$BERYL_CACHE/ladder/positions`, with its source seed, turn and both estimates. A
//!   fixed seed rebuilds the same set, whose hash the build prints.
//! - `beryl_cli positions sample [--count N] DIR` prints N positions, spread over the set, as
//!   transcripts, for a human to read.
//!
//! A game starts from a position as a rebuilt save does ([`start`]): a new game of its seed,
//! arranged as the board stood just before the turn began, the library cards no player knows
//! shuffled by the seed. So the turn begins again, untap, upkeep and draw, as it did. A library
//! card some player knows (a mulligan's bottom card, a scry's top one) stays where it was and
//! known to them, so the reshuffle makes nothing either player knows false: most cuts hold one,
//! since a London mulligan's bottom card stays known to its owner.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, GameTable, Profile, Replay, Scratch, State, Step, apply, next, next_in_step, observe,
    view,
};
use beryl_search::{Agent, DEEP_SEARCHER, LongSearch, Settings};
use beryl_session::checkpoint::Checkpoint;
use serde::{Deserialize, Serialize};

use crate::game::{mirror, parse_work, seat_seeds};
use crate::ladder::games_at_once;
use crate::match_log::{cache, files};
use crate::tournament::game_seed;
use crate::transcript::Transcript;

/// The turns cut, as each begins.
const TURNS: std::ops::RangeInclusive<u16> = 3..=8;

/// The champion's estimates a position is kept within, for the seat to move.
const BAND: [f64; 2] = [0.45, 0.55];

/// Turns past the cut a scoring search may wait for each seat's first real decision.
const SCORE_TURNS: u16 = 2;

const USAGE: &str = "usage: beryl_cli positions build [--games N] [--seed N] [--work 3M] \
                     [--out DIR]\n       beryl_cli positions sample [--count N] DIR";

/// A start position, as its file keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    /// The seed of the game it was cut from, and the turn that was beginning.
    pub seed: u64,
    pub turn: u16,
    /// The champion's estimate of its chances from each seat, by seat.
    pub estimates: [f64; 2],
    /// The board just before the turn began.
    pub board: Checkpoint,
}

/// How a set is built.
#[derive(Clone, Copy, Debug)]
pub struct Build {
    pub games: u32,
    pub seed: u64,
    /// The champion's settings, on one thread: games run one to a core.
    pub settings: Settings,
    pub band: [f64; 2],
}

/// What a build found.
#[derive(Debug, Default)]
pub struct Built {
    /// The kept positions by file name, in order.
    pub positions: Vec<(String, Position)>,
    /// Turn starts seen in the cut turns, those clean enough to reshuffle, and those scored
    /// (both seats searched before the scoring gave up).
    pub cuts: u32,
    pub clean: u32,
    pub scored: u32,
}

pub fn main(args: &[String]) -> Result<(), String> {
    match args {
        [command, rest @ ..] if command == "build" => build_main(rest),
        [command, rest @ ..] if command == "sample" => sample_main(rest),
        _ => Err(USAGE.to_owned()),
    }
}

fn build_main(args: &[String]) -> Result<(), String> {
    let mut b = Build {
        games: 250,
        seed: 0,
        settings: Settings {
            family: DEEP_SEARCHER,
            work: 3_000_000,
            threads: 1,
        },
        band: BAND,
    };
    let mut out = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))?;
        match arg.as_str() {
            "--games" => b.games = crate::number(arg, value)?,
            "--seed" => b.seed = crate::number(arg, value)?,
            "--work" => {
                b.settings.work =
                    parse_work(value).ok_or_else(|| format!("--work: no budget {value:?}"))?;
            }
            "--out" => out = Some(PathBuf::from(value)),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let out = out.unwrap_or_else(default_dir);
    if fs::read_dir(&out).is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!(
            "{} is not empty: pass another --out",
            out.display()
        ));
    }
    let built = build(&b, |done| eprintln!("game {done} of {} cut", b.games))?;
    write(&out, &built.positions)?;
    println!(
        "{} games: {} turn starts cut, {} clean, {} scored, {} kept in [{}, {}]; set {:016x} in {}",
        b.games,
        built.cuts,
        built.clean,
        built.scored,
        built.positions.len(),
        b.band[0],
        b.band[1],
        hash(&built.positions)?,
        out.display()
    );
    Ok(())
}

fn sample_main(args: &[String]) -> Result<(), String> {
    let (count, dir) = match args {
        [flag, n, dir] if flag == "--count" => (crate::number(flag, n)?, dir),
        [dir] => (10, dir),
        _ => return Err(USAGE.to_owned()),
    };
    print!("{}", sample(Path::new(dir), count)?);
    Ok(())
}

/// Where a set lives unless told otherwise.
pub fn default_dir() -> PathBuf {
    cache().join("ladder").join("positions")
}

/// Whether a game can start from a cut: nothing lasts past the turn's beginning off the board,
/// which a checkpoint does not record ([`Checkpoint::lingering`]).
pub fn clean(board: &Checkpoint) -> bool {
    board.lingering.is_none()
}

/// The state a game of `seed` starts from at `position`, before its turn begins.
pub fn start(t: &GameTable, position: &Position, seed: u64) -> Result<State, String> {
    let mut s = State::new(t, seed, Profile::SMART).map_err(|e| e.to_string())?;
    position
        .board
        .arrange(t, &mut s, seed)
        .map_err(|e| format!("a position of turn {}: {e}", position.turn))?;
    Ok(s)
}

/// Builds a set, calling `progress` with the games cut so far as each finishes.
pub fn build(b: &Build, progress: impl Fn(u32) + Sync) -> Result<Built, String> {
    let t = mirror();
    let next_game = AtomicU32::new(0);
    let done = AtomicU32::new(0);
    let found: Mutex<Vec<(u32, Result<GameCuts, String>)>> = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..games_at_once().min(b.games as usize) {
            scope.spawn(|| {
                loop {
                    let g = next_game.fetch_add(1, Ordering::Relaxed);
                    if g >= b.games {
                        break;
                    }
                    let cut = cut_game(&t, b, g);
                    found.lock().unwrap().push((g, cut));
                    progress(done.fetch_add(1, Ordering::Relaxed) + 1);
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap();
    found.sort_by_key(|(g, _)| *g);
    let mut built = Built::default();
    for (_, cuts) in found {
        let cuts = cuts?;
        built.cuts += cuts.cuts;
        built.clean += cuts.clean;
        built.scored += cuts.scored;
        built.positions.extend(cuts.kept);
    }
    Ok(built)
}

/// One game's cuts.
struct GameCuts {
    cuts: u32,
    clean: u32,
    scored: u32,
    kept: Vec<(String, Position)>,
}

/// Plays game `g` of a build to the end of the cut turns, cutting and scoring as each begins.
fn cut_game(t: &GameTable, b: &Build, g: u32) -> Result<GameCuts, String> {
    let seed = game_seed(b.seed, 0, g);
    let seeds = seat_seeds(seed);
    let mut agents = seeds.map(|s| LongSearch::<beryl_engine::Small>::new(t, b.settings, s));
    let mut s = State::new(t, seed, Profile::SMART).map_err(|e| e.to_string())?;
    let mut x = Scratch::new();
    let mut at = (s.turn().number(), s.turn().step());
    let mut out = GameCuts {
        cuts: 0,
        clean: 0,
        scored: 0,
        kept: Vec::new(),
    };
    loop {
        match next_in_step(&mut s, t, &mut x, &mut Replay, at) {
            None => {
                let turn = s.turn();
                // A turn is seen as its upkeep begins, as a save's checkpoint is taken.
                if turn.number() != at.0 && TURNS.contains(&turn.number()) {
                    out.cuts += 1;
                    let board = Checkpoint::of(t, &s);
                    if clean(&board) {
                        out.clean += 1;
                        let score_seed =
                            SplitMix64::new(seed ^ u64::from(turn.number())).next_u64();
                        let mut position = Position {
                            seed,
                            turn: turn.number(),
                            estimates: [0.0; 2],
                            board,
                        };
                        if let Some(estimates) = estimates(t, &position, score_seed, b.settings)? {
                            out.scored += 1;
                            position.estimates = estimates;
                            let active = usize::from(position.board.active);
                            let for_active = [estimates[active], 1.0 - estimates[1 - active]];
                            if for_active.iter().all(|&e| b.band[0] <= e && e <= b.band[1]) {
                                out.kept
                                    .push((format!("g{g:04}-t{}.json", turn.number()), position));
                            }
                        }
                    }
                }
                if turn.number() > *TURNS.end() {
                    break;
                }
                at = (turn.number(), turn.step());
            }
            Some(Step::Over(_)) => break,
            Some(Step::Decide(d)) => {
                let c = agents[d.player.index()].choose(observe(&s, t, &mut x, d.player), &d);
                apply(&mut s, t, &mut x, &mut Replay, c);
            }
        }
    }
    Ok(out)
}

/// The champion's estimate of `position` from each seat: the mean of the choice of its search
/// at each seat's first decision with more than one candidate, played on from the position
/// with the libraries shuffled by `seed`. None if a seat has no such decision within
/// [`SCORE_TURNS`] turns.
fn estimates(
    t: &GameTable,
    position: &Position,
    seed: u64,
    settings: Settings,
) -> Result<Option<[f64; 2]>, String> {
    let mut s = start(t, position, seed)?;
    let mut x = Scratch::new();
    let mut searches =
        seat_seeds(seed).map(|s| LongSearch::<beryl_engine::Small>::new(t, settings, s));
    let mut found: [Option<f64>; 2] = [None; 2];
    while s.turn().number() <= position.turn + SCORE_TURNS {
        let Step::Decide(d) = next(&mut s, t, &mut x, &mut Replay) else {
            return Ok(None);
        };
        let seat = d.player.index();
        let c = searches[seat].choose(observe(&s, t, &mut x, d.player), &d);
        if found[seat].is_none()
            && d.len() > 1
            && let Some(search) = Agent::last_search(&searches[seat])
        {
            found[seat] = search.means.get(usize::from(c.0)).copied().flatten();
        }
        if let [Some(a), Some(b)] = found {
            return Ok(Some([a, b]));
        }
        apply(&mut s, t, &mut x, &mut Replay, c);
    }
    Ok(None)
}

/// Writes a set's positions into `dir`.
pub fn write(dir: &Path, positions: &[(String, Position)]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (name, p) in positions {
        let json = serde_json::to_string_pretty(p).map_err(|e| e.to_string())? + "\n";
        files::write_whole(&dir.join(name), json.as_bytes())?;
    }
    Ok(())
}

/// The positions of the set in `dir`, by file name, in order.
pub fn load(dir: &Path) -> Result<Vec<(String, Position)>, String> {
    let at = |e: std::io::Error| format!("{}: {e}", dir.display());
    let mut names: Vec<String> = fs::read_dir(dir)
        .map_err(at)?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()
        .map_err(at)?;
    names.retain(|n| Path::new(n).extension().is_some_and(|e| e == "json"));
    names.sort();
    if names.is_empty() {
        return Err(format!("{} holds no positions", dir.display()));
    }
    names
        .into_iter()
        .map(|name| {
            let path = dir.join(&name);
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let p = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok((name, p))
        })
        .collect()
}

/// A hash of a set's files, FNV-1a over names and contents in order.
pub fn hash(positions: &[(String, Position)]) -> Result<u64, String> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, p) in positions {
        let json = serde_json::to_string(p).map_err(|e| e.to_string())?;
        for byte in name.bytes().chain(json.bytes()) {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    Ok(h)
}

/// `count` positions of the set in `dir`, spread evenly over it, as transcripts: each board as
/// its turn begins and the first decision greedy answers there.
pub fn sample(dir: &Path, count: usize) -> Result<String, String> {
    let set = load(dir)?;
    let t = mirror();
    let step = (set.len() as f64 / count.max(1) as f64).max(1.0);
    let mut out = String::new();
    for k in 0..count.min(set.len()) {
        let (name, p) = &set[(k as f64 * step) as usize];
        let _ = writeln!(
            out,
            "== {name}: game seed {}, turn {}, player {} active; estimates {:.3} (player 0), {:.3} \
             (player 1)",
            p.seed, p.turn, p.board.active, p.estimates[0], p.estimates[1]
        );
        let mut s = start(&t, p, p.seed)?;
        let mut x = Scratch::recording();
        let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
            out.push_str("(the game ends before a decision)\n\n");
            continue;
        };
        let info = beryl_search::agent("greedy").ok_or("no greedy agent")?;
        let mut greedy = (info.new)(&t, p.seed, beryl_search::Budget::DEFAULT);
        let c: Choice = Agent::<beryl_engine::Small>::choose(
            &mut greedy,
            observe(&s, &t, &mut x, d.player),
            &d,
        );
        let happened: Vec<_> = x.events.drain().collect();
        let mut transcript = Transcript::new(["player 0", "player 1"]);
        transcript.decision(&t, view(&s, &t, &mut x), &d, c, &happened);
        out.push_str(transcript.text());
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Build {
        Build {
            games: 2,
            seed: 5,
            settings: Settings {
                family: "pimc2/hidden/rollout/tuned".parse().unwrap(),
                work: 2_000,
                threads: 1,
            },
            band: [0.0, 1.0],
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("beryl-positions-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// A fixed seed rebuilds the same set; every position loads from its file and plays on
    /// to the end of the game.
    #[test]
    fn a_seed_rebuilds_its_set_and_each_position_plays_on() {
        let built = build(&tiny(), |_| {}).unwrap();
        assert!(!built.positions.is_empty(), "{built:?}");
        assert!(built.positions.len() as u32 <= built.scored);
        assert!(built.scored <= built.clean && built.clean <= built.cuts);
        let again = build(&tiny(), |_| {}).unwrap();
        assert_eq!(hash(&built.positions), hash(&again.positions));
        let dir = scratch("set");
        write(&dir, &built.positions).unwrap();
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded, built.positions);
        let t = mirror();
        let info = beryl_search::agent("greedy").unwrap();
        for (name, p) in &loaded {
            assert!(clean(&p.board), "{name}");
            assert!(TURNS.contains(&p.turn), "{name}");
            let mut s = start(&t, p, 99).unwrap();
            assert_eq!(
                s.turn().number(),
                p.turn - 1,
                "{name}: before the turn begins"
            );
            let mut agents = [0, 1].map(|i| (info.new)(&t, i, beryl_search::Budget::DEFAULT));
            let mut x = Scratch::new();
            let mut decisions = 0;
            loop {
                match next(&mut s, &t, &mut x, &mut Replay) {
                    Step::Over(_) => break,
                    Step::Decide(d) => {
                        let c = Agent::<beryl_engine::Small>::choose(
                            &mut agents[d.player.index()],
                            observe(&s, &t, &mut x, d.player),
                            &d,
                        );
                        apply(&mut s, &t, &mut x, &mut Replay, c);
                        decisions += 1;
                        assert!(decisions < 5_000, "{name}: the game never ends");
                    }
                }
            }
        }
        assert!(sample(&dir, 2).unwrap().contains("=="));
        let _ = fs::remove_dir_all(&dir);
    }

    /// A cut where something lasts past the turn's beginning is rejected; a library card a
    /// player knows stays where it was, known to them alone, when a game starts there.
    #[test]
    fn lingering_rejects_a_cut_and_known_cards_stay_known() {
        let mut b = tiny();
        b.games = 4;
        let built = build(&b, |_| {}).unwrap();
        let (name, p) = built
            .positions
            .iter()
            .find(|(_, p)| p.board.players.iter().any(|side| !side.known.is_empty()))
            .expect("a position with a known library card");
        let t = mirror();
        for seed in [1, 2] {
            let s = start(&t, p, seed).unwrap();
            let again = Checkpoint::of(&t, &s);
            for (side, now) in p.board.players.iter().zip(&again.players) {
                assert_eq!(side.known, now.known, "{name}, seed {seed}");
            }
        }
        let mut lingering = p.board.clone();
        assert!(clean(&lingering));
        lingering.lingering = Some("an effect until your next turn".to_owned());
        assert!(!clean(&lingering));
    }
}
