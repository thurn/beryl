//! `beryl_cli freeze-saves <dir> [--games N] [--seed N] [--replay FILE]`: adds saves to the
//! frozen corpus at `<dir>` (`beryl_session::corpus`, `just freeze-saves`).
//!
//! It plays `N` seeded games (200 by default) with the cheap agents in both seats, the human's
//! answered by a stand-in, between fuzz decks and, every eighth game, the vintage WU Control
//! deck against one. It keeps a small set of them that between them play or cast every card of
//! the cube and of that deck, or activate an ability of it, and make every kind of decision,
//! as far as the games reached; prints the coverage it reached; checks that each kept game
//! replays exactly; and freezes each as `<date>-<seed>.json.gz`. The seeds begin from today's
//! date unless `--seed` says, so each day's run adds new games. Run it every week or so, so
//! the corpus holds saves of every age.
//!
//! `--replay FILE` freezes that replay file instead (a bug report's, a game played in the app)
//! as `<date>-<its name>.json.gz`, once it replays exactly.

use std::collections::BTreeSet;
use std::path::Path;
use std::thread;

use beryl_cards::CARDS;
use beryl_cards::decks::{VINTAGE_WU_CONTROL, fuzz_deck};
use beryl_engine::card::Supertypes;
use beryl_session::corpus;
use beryl_session::replay::Entry;
use beryl_session::save::{Act, Kind};
use beryl_session::{Game, ReplayFile, Seat, SessionId, StartGame, Status, StopsSetting};

const USAGE: &str = "usage: beryl_cli freeze-saves <dir> [--games N] [--seed N] [--replay FILE]";

/// The cube's cardlist, `color,name` rows with a header.
const CUBE: &str = include_str!("../../../../data/arena-powered-cube-2025-10-30.csv");

/// The agents that play the games: cheap ones, which search nothing.
const AGENTS: [&str; 2] = ["greedy", "random"];

/// The human's decisions a game is played for at most.
const DECISIONS: usize = 3000;

pub fn main(args: &[String]) -> Result<(), String> {
    let [dir, flags @ ..] = args else {
        return Err(USAGE.to_owned());
    };
    let dir = Path::new(dir);
    let (mut games, mut seed, mut replay) = (200, None, None);
    for pair in flags.chunks(2) {
        let [flag, value] = pair else {
            return Err(USAGE.to_owned());
        };
        let number = || value.parse::<u64>().map_err(|_| USAGE.to_owned());
        match flag.as_str() {
            "--games" => games = number()?,
            "--seed" => seed = Some(number()?),
            "--replay" => replay = Some(Path::new(value)),
            _ => return Err(USAGE.to_owned()),
        }
    }
    if let Some(path) = replay {
        return freeze_replay(dir, path);
    }
    // Days since the epoch, so each day's run plays games of its own.
    let seed = seed.unwrap_or_else(|| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        secs / 86_400 * 1_000_000
    });
    freeze_games(dir, seed, games)
}

/// Freezes the replay file at `path`.
fn freeze_replay(dir: &Path, path: &Path) -> Result<(), String> {
    let mut file = ReplayFile::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ending = corpus::check(&CARDS, &file, None)
        .map_err(|f| format!("{} does not replay exactly: {f}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: no file name", path.display()))?;
    if file.commit.is_none() {
        file.commit = head();
    }
    let name = format!("{}-{stem}.json.gz", corpus::today());
    corpus::freeze(dir, &name, &file, ending)?;
    println!("froze {name}, which ends {ending}");
    Ok(())
}

/// One game played for the corpus.
struct Played {
    seed: u64,
    file: ReplayFile,
    covers: BTreeSet<Target>,
}

/// What the corpus covers: a card played, cast or activated, a kind of decision made, or a
/// game of the vintage WU Control deck.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Card(String),
    Kind(Kind),
    Vintage,
}

fn freeze_games(dir: &Path, seed: u64, games: u64) -> Result<(), String> {
    let targets = targets();
    let threads = thread::available_parallelism().map_or(4, usize::from) as u64;
    let played: Vec<Played> = thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    (t..games)
                        .step_by(threads as usize)
                        .map(|i| play(seed + i, i))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a game panicked"))
            .collect()
    });
    let mut kept = cover(&played, &targets);
    kept.sort_by_key(|p| p.seed);
    report(&kept, &targets, games);
    let commit = head();
    let date = corpus::today();
    for p in kept {
        let ending = corpus::check(&CARDS, &p.file, None)
            .map_err(|f| format!("the game of seed {} does not replay exactly: {f}", p.seed))?;
        let mut file = p.file.clone();
        file.commit.clone_from(&commit);
        let name = format!("{date}-{}.json.gz", p.seed);
        corpus::freeze(dir, &name, &file, ending)?;
        println!("froze {name}, which ends {ending}");
    }
    Ok(())
}

/// Every card of the cube and of the vintage WU Control deck implemented so far, but the
/// basic lands, every kind of decision and a game of that deck.
fn targets() -> BTreeSet<Target> {
    let vintage = VINTAGE_WU_CONTROL.iter().map(|&(_, name)| name);
    cube()
        .chain(vintage)
        .filter_map(|name| CARDS.get(name))
        .filter(|c| !c.front().supertypes.contains(Supertypes::BASIC))
        .map(|c| Target::Card(c.name.to_owned()))
        .chain(Kind::ALL.into_iter().map(Target::Kind))
        .chain([Target::Vintage])
        .collect()
}

/// The names of the cube's cards.
fn cube() -> impl Iterator<Item = &'static str> {
    CUBE.lines().skip(1).map(|row| {
        let (_, name) = row.split_once(',').expect("a color,name row");
        name.trim_matches('"')
    })
}

/// Prints what the games `kept` of `games` cover of `targets`, and what they miss.
fn report(kept: &[&Played], targets: &BTreeSet<Target>, games: u64) {
    let covered: BTreeSet<&Target> = kept.iter().flat_map(|p| &p.covers).collect();
    let of = |kind: fn(&Target) -> bool| -> Vec<&Target> {
        targets.iter().filter(|t| kind(t)).collect()
    };
    let cards = of(|t| matches!(t, Target::Card(_)));
    let kinds = of(|t| matches!(t, Target::Kind(_)));
    let reached = |ts: &[&Target]| ts.iter().filter(|t| covered.contains(*t)).count();
    let unimplemented = cube().filter(|name| CARDS.get(name).is_none()).count();
    println!(
        "kept {} of {games} games: {} of {} cards, {} of {} kinds of decision \
         ({unimplemented} of the cube's cards are not implemented yet)",
        kept.len(),
        reached(&cards),
        cards.len(),
        reached(&kinds),
        kinds.len(),
    );
    let missing: Vec<String> = targets
        .iter()
        .filter(|t| !covered.contains(t))
        .map(|t| match t {
            Target::Card(name) => name.clone(),
            Target::Kind(kind) => format!("{kind:?} decisions"),
            Target::Vintage => "a game of the vintage WU Control deck".to_owned(),
        })
        .collect();
    if !missing.is_empty() {
        println!("not reached: {}", missing.join("; "));
    }
}

/// Game `i` of a run, from `seed`: who plays it varies with `i`.
fn play(seed: u64, i: u64) -> Played {
    let owned = |deck: &[(u8, &str)]| -> Vec<(u8, String)> {
        deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
    };
    let first = if i.is_multiple_of(8) {
        owned(VINTAGE_WU_CONTROL)
    } else {
        owned(&fuzz_deck(seed))
    };
    let start = StartGame {
        seed,
        decks: [first, owned(&fuzz_deck(seed ^ 1 << 40))],
        human: Seat::P0,
        opponent: AGENTS[(i / 2 % 2) as usize].to_owned(),
        stops: if i.is_multiple_of(3) {
            StopsSetting::SMART
        } else {
            StopsSetting::EXACT
        },
    };
    let mut game = Game::new(&CARDS, &start, SessionId(0)).expect("a fuzz game starts");
    game.set_stand_in(AGENTS[(i % 2) as usize])
        .expect("a registered agent");
    for _ in 0..DECISIONS {
        let mut over = false;
        while let Some(view) = game.next_view() {
            over = matches!(view.status, Status::Over { .. });
        }
        if over || !game.answer_by_stand_in() {
            break;
        }
    }
    while game.next_view().is_some() {}
    let file = game.replay().clone();
    let mut covers = covers(&file);
    if i.is_multiple_of(8) {
        covers.insert(Target::Vintage);
    }
    Played { seed, file, covers }
}

/// The cards `file`'s game played, cast or activated, and the kinds of decision it made.
fn covers(file: &ReplayFile) -> BTreeSet<Target> {
    let mut covers = BTreeSet::new();
    for entry in &file.log {
        let (Entry::Choice(c) | Entry::Auto(c)) = entry else {
            continue;
        };
        let Some(what) = &c.what else {
            continue;
        };
        covers.insert(Target::Kind(what.kind));
        match &what.act {
            Act::PlayLand { card } | Act::Cast { card, .. } | Act::Activate { card, .. } => {
                covers.insert(Target::Card(card.name.clone()));
            }
            Act::Mana { source, .. } => {
                covers.insert(Target::Card(source.name.clone()));
            }
            _ => {}
        }
    }
    covers
}

/// A small set of `played`, cheap to replay, covering as many of `targets` as they do between
/// them: each pick the game that covers the most not covered yet for the length of its log.
fn cover<'a>(played: &'a [Played], targets: &BTreeSet<Target>) -> Vec<&'a Played> {
    let mut left: BTreeSet<&Target> = targets.iter().collect();
    let mut kept = Vec::new();
    loop {
        let gain = |p: &Played| p.covers.iter().filter(|t| left.contains(t)).count();
        let best = played.iter().filter(|p| gain(p) > 0).max_by(|a, b| {
            // The gain for the length, `gain(a) / len(a)` against `gain(b) / len(b)`.
            (gain(a) * b.file.log.len())
                .cmp(&(gain(b) * a.file.log.len()))
                .then(b.seed.cmp(&a.seed))
        });
        let Some(p) = best else {
            return kept;
        };
        for t in &p.covers {
            left.remove(t);
        }
        kept.push(p);
    }
}

/// The commit checked out, if git says.
fn head() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
