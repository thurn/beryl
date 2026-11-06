//! The long-think ladder (the by-2l8 epic): an open-ended tournament of the hidden search at
//! budgets tripling rung by rung, against a fixed opponent, which any stop costs at most the
//! decision being searched.
//!
//! `beryl_cli longthink` (`just longthink`) starts the ladder in a new run directory under
//! `$BERYL_CACHE/longthink`, or resumes the latest, and supervises the runner: the binary
//! the run was started with, copied into its directory, so later commits never change a
//! run's agents. A runner that crashes is restarted and resumes. `--games N` stops once the
//! run holds N games, however many sessions it took; `--status` prints the summary;
//! `--upgrade` adopts the current code if it plays identically ([`upgrade`]); `--new` and
//! `--run DIR` start a new run, or resume any. Its files:
//!
//! - `config.json`, the run's settings ([`Config`]), and `code.json`, the git tree ids of the
//!   crates that decide how the run plays, when the binary was built;
//! - `games.jsonl`, a line per finished game ([`GameRecord`]), the record that commits it;
//! - `decisions.jsonl`, a line per searched decision ([`DecisionRecord`]): the root's
//!   visits and mean rewards, training data, since a game replays from its seed;
//! - `clock.jsonl`, each game's wall seconds, kept apart so the other logs are a function
//!   of the seeds alone;
//! - `logs/<game>.jsonl`, every decision's answer as it was made ([`game::Line`]), which a
//!   resumed game replays through, and `transcripts/<game>.txt`;
//! - `crashes.jsonl`, where the runner was when it panicked;
//! - `summary.txt`, rewritten after every game and rebuilt when missing.
//!
//! The runner shows the run on a live panel ([`term`]), which `--status` prints once.

mod files;
mod game;
mod panel;
mod run;
mod schedule;
mod standings;
mod summary;
mod term;
mod upgrade;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use beryl_search::{AgentDecision, Budget, Sight};
use serde::{Deserialize, Serialize};

use game::{Entrant, GameDecision};
pub use run::run;
use schedule::{Against, Config, Matchup};

/// The crates whose code decides how a run plays.
const CRATES: [&str; 3] = ["beryl_engine", "beryl_cards", "beryl_search"];

/// How a finished game went for the rung's agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Win,
    Loss,
    Draw,
    Stalled,
    /// The runner crashed twice at the same decision: skipped, for a bug report.
    Crashed,
}

/// A line of `games.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameRecord {
    pub game: String,
    #[serde(flatten)]
    pub matchup: Matchup,
    pub iterations: u32,
    pub pair: u32,
    pub seed: u64,
    /// The seat of the rung's agent.
    pub seat: u8,
    pub outcome: Verdict,
    pub turns: u16,
    pub decisions: u32,
    /// Decisions searched, by either seat, and the rollouts they spent.
    pub searched: u32,
    pub rollouts: u64,
    /// What the searched decisions' budgets allowed, the rollouts they would have spent
    /// with no exact stop and no subtree reuse.
    #[serde(default)]
    pub budget: u64,
    /// The decision a crashed game crashed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crashed_at: Option<GameDecision>,
}

/// A line of `decisions.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub game: String,
    pub decision: GameDecision,
    pub seat: u8,
    pub agent: String,
    pub iterations: u32,
    pub choice: u8,
    /// The decision's index among the agent's own, and that of the decision whose search
    /// planted the trees it grew.
    pub agent_decision: AgentDecision,
    pub chain: AgentDecision,
    pub rollouts: u32,
    pub inherited: u32,
    pub visits: Vec<u32>,
    /// Each candidate's mean reward for the deciding player; none if never visited.
    pub mean: Vec<Option<f64>>,
}

/// A line of `clock.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClockRecord {
    pub game: String,
    pub seconds: f64,
}

/// A budget, short: 3k, 1M.
pub fn rung_name(iterations: u32) -> String {
    match iterations {
        0 => "0".to_owned(),
        n if n % 1_000_000 == 0 => format!("{}M", n / 1_000_000),
        n if n % 1_000 == 0 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

/// Game `g` of a matchup: its key, seed and agents.
#[derive(Clone, Debug)]
pub struct Spec {
    pub key: String,
    pub matchup: Matchup,
    pub pair: u32,
    pub seed: u64,
    /// The seat of the rung's agent.
    pub seat: u8,
    /// The agents, by seat.
    pub entrants: [Entrant; 2],
}

impl Spec {
    /// Game `g` of matchup `m`: pair `g / 2`, the rung's agent in seat `g % 2`.
    pub fn new(c: &Config, m: Matchup, g: u32) -> Spec {
        let Matchup { rung, against } = m;
        let search = |sight, rung: usize| Entrant::Search {
            sight,
            budget: Budget {
                iterations: c.iterations(rung),
                trees: c.trees,
                parallel: true,
            },
        };
        let opponent = Entrant::Fixed(c.opponent.clone());
        let (a, b) = match against {
            Against::Opponent => (search(Sight::Hidden, rung), opponent),
            Against::Omniscient => (search(Sight::Omniscient, rung), opponent),
            Against::Below => (search(Sight::Hidden, rung), search(Sight::Hidden, rung - 1)),
        };
        let (pair, seat) = (g / 2, (g % 2) as u8);
        Spec {
            key: format!("{}-p{pair:03}-s{seat}", m.key(c)),
            matchup: m,
            pair,
            seed: crate::tournament::game_seed(c.seed, 0, pair),
            seat,
            entrants: if seat == 0 { [a, b] } else { [b, a] },
        }
    }
}

#[cfg(test)]
impl GameRecord {
    /// Game `g` of matchup `m`, ended `outcome` for the rung's agent.
    fn example(m: Matchup, g: u32, outcome: Verdict) -> GameRecord {
        GameRecord {
            game: format!("{}-{:?}-{g}", m.rung, m.against),
            matchup: m,
            iterations: 0,
            pair: g / 2,
            seed: 0,
            seat: (g % 2) as u8,
            outcome,
            turns: 20,
            decisions: 300,
            searched: 130,
            rollouts: 1,
            budget: 0,
            crashed_at: None,
        }
    }
}

const USAGE: &str =
    "usage: beryl_cli longthink [--status | --upgrade] [--new | --run DIR] [--games N]
  a new run also takes: [--rungs N,N,...] [--cheap N] [--pairs N] [--trees K]
      [--opponent NAME] [--seed N]";

/// `beryl_cli longthink`: starts the ladder or resumes it, and supervises it.
pub fn main(args: &[String]) -> Result<(), String> {
    let mut new = false;
    let mut status = false;
    let mut upgrade = false;
    let mut at: Option<PathBuf> = None;
    let mut games: Option<u32> = None;
    let mut c = Config::default();
    let mut configured = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--new" => new = true,
            "--status" => status = true,
            "--upgrade" => upgrade = true,
            "--run" => at = Some(PathBuf::from(value()?)),
            "--games" => games = Some(crate::number(arg, &value()?)?),
            flag => {
                configured = true;
                match flag {
                    "--rungs" => {
                        c.rungs = value()?
                            .split(',')
                            .map(|n| crate::number(arg, n))
                            .collect::<Result<_, _>>()?;
                    }
                    "--cheap" => c.cheap = crate::number(arg, &value()?)?,
                    "--pairs" => c.pairs = crate::number(arg, &value()?)?,
                    "--trees" => c.trees = crate::number(arg, &value()?)?,
                    "--opponent" => c.opponent = value()?,
                    "--seed" => c.seed = crate::number(arg, &value()?)?,
                    _ => return Err(format!("unknown argument {flag}\n{USAGE}")),
                }
            }
        }
    }
    if c.rungs.is_empty() || c.rungs.contains(&0) || c.pairs == 0 || c.trees == 0 {
        return Err("rungs, pairs and trees must be positive".to_owned());
    }
    if beryl_search::agent(&c.opponent).is_none() {
        return Err(format!("no agent is named {:?}", c.opponent));
    }
    let root = cache().join("longthink");
    let dir = match at {
        Some(dir) => dir,
        None if new => fresh_dir(&root)?,
        None => match latest(&root)? {
            Some(dir) => dir,
            None if status => return Err(format!("no run in {}", root.display())),
            None => fresh_dir(&root)?,
        },
    };
    let started = dir.join("config.json").exists();
    if status {
        summary(&dir)?;
        term::status(
            &run::run_name(&dir),
            config(&dir)?,
            files::read(&dir.join("games.jsonl"))?,
            run::clock(&dir)?,
        );
        return Ok(());
    }
    let code = code_ids(&repo());
    if started {
        if configured {
            return Err(format!(
                "{} has its settings; they apply to a new run (--new)",
                dir.display()
            ));
        }
        let games = files::read::<GameRecord>(&dir.join("games.jsonl"))?.len();
        println!("resuming {} ({games} games)", dir.display());
        let theirs: Option<BTreeMap<String, String>> = fs::read_to_string(dir.join("code.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        let differ = changed(theirs.as_ref(), code.as_ref());
        if !differ.is_empty() {
            println!(
                "the code differs from the run's in {}: it plays on with its own binary \
                 (--upgrade adopts the code if it plays identically, --new starts a new run)",
                differ.join(", ")
            );
        }
    } else {
        start(&dir, &c, code.as_ref())?;
        println!("new run {}", dir.display());
    }
    if upgrade {
        return upgrade::adopt(&dir, code.as_ref());
    }
    supervise(&dir, games)
}

/// The per-machine cache: `$BERYL_CACHE`, else `~/Library/Caches/beryl`.
fn cache() -> PathBuf {
    match std::env::var_os("BERYL_CACHE") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Caches/beryl")
        }
    }
}

/// The checkout this binary was built in, four up from it in `rust/target/<profile>/`. Not
/// `env!("CARGO_MANIFEST_DIR")`: a `rust/target` cloned into another worktree
/// (`tools/src/beryl_tools/warm.py`) keeps binaries cargo counts as fresh, since it
/// fingerprints workspace members by relative path, compiled with the old worktree's path.
fn repo() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.ancestors().nth(4).map(Path::to_path_buf))
        .expect("the checkout")
}

/// The run directories under `root` and their numbers, oldest first: `run-001`, `run-002`,
/// ...
fn runs(root: &Path) -> Result<Vec<(u32, PathBuf)>, String> {
    let mut runs: Vec<(u32, PathBuf)> = match fs::read_dir(root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let n = e.file_name().to_str()?.strip_prefix("run-")?.parse().ok()?;
                Some((n, e.path()))
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("{}: {e}", root.display())),
    };
    runs.sort();
    Ok(runs)
}

fn latest(root: &Path) -> Result<Option<PathBuf>, String> {
    Ok(runs(root)?.pop().map(|(_, p)| p))
}

/// The directory a new run gets: numbered after the latest, so it becomes the latest.
fn fresh_dir(root: &Path) -> Result<PathBuf, String> {
    let n = runs(root)?.last().map_or(0, |&(n, _)| n) + 1;
    Ok(root.join(format!("run-{n:03}")))
}

/// Starts a run in `dir`: its settings, the code it plays, and its own copy of this binary.
fn start(dir: &Path, c: &Config, code: Option<&BTreeMap<String, String>>) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    adopt_binary(dir, code)?;
    let json = serde_json::to_string_pretty(c).map_err(|e| e.to_string())? + "\n";
    // The settings come last: a run without them has not started.
    files::write_whole(&dir.join("config.json"), json.as_bytes())
}

/// Copies this binary into run `dir` as the one it plays with, and records its code.
fn adopt_binary(dir: &Path, code: Option<&BTreeMap<String, String>>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("this binary: {e}"))?;
    let tmp = dir.join("beryl_cli.tmp");
    fs::copy(&exe, &tmp).map_err(|e| format!("copying {}: {e}", exe.display()))?;
    fs::rename(&tmp, dir.join("beryl_cli")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let json = serde_json::to_string_pretty(&code).map_err(|e| e.to_string())? + "\n";
    files::write_whole(&dir.join("code.json"), json.as_bytes())
}

/// The run's settings.
fn config(dir: &Path) -> Result<Config, String> {
    let path = dir.join("config.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The run's summary, rebuilt from its logs if missing.
fn summary(dir: &Path) -> Result<String, String> {
    match fs::read_to_string(dir.join("summary.txt")) {
        Ok(text) => Ok(text),
        Err(_) => run::write_summary(dir, &config(dir)?, &files::read(&dir.join("games.jsonl"))?),
    }
}

/// The crates whose code differs between two sets of tree ids, all if either is unknown.
fn changed(
    a: Option<&BTreeMap<String, String>>,
    b: Option<&BTreeMap<String, String>>,
) -> Vec<&'static str> {
    CRATES
        .into_iter()
        .filter(|c| match (a, b) {
            (Some(a), Some(b)) => a.get(*c) != b.get(*c),
            _ => true,
        })
        .collect()
}

/// The git tree ids of the crates that decide how a run plays, uncommitted edits included,
/// in checkout `repo`; none outside a checkout.
fn code_ids(repo: &Path) -> Option<BTreeMap<String, String>> {
    let git = |index: &Path, args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .env("GIT_INDEX_FILE", index)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    // A copy of the index, so staging the working tree touches nothing of the checkout's.
    let real = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--path-format=absolute", "--git-path", "index"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let real = PathBuf::from(String::from_utf8_lossy(&real.stdout).trim());
    let index = std::env::temp_dir().join(format!("beryl-longthink-index-{}", std::process::id()));
    let _ = fs::copy(&real, &index);
    let paths: Vec<String> = CRATES.iter().map(|c| format!("rust/crates/{c}")).collect();
    let mut add = vec!["add", "-A", "--"];
    add.extend(paths.iter().map(String::as_str));
    let ids = git(&index, &add).and_then(|_| {
        CRATES
            .iter()
            .map(|c| {
                let id = git(
                    &index,
                    &["write-tree", &format!("--prefix=rust/crates/{c}/")],
                )?;
                Some(((*c).to_owned(), id))
            })
            .collect()
    });
    let _ = fs::remove_file(&index);
    ids
}

/// Runs the run's own binary until it finishes, restarting it when it crashes. The terminal
/// is the runner's: Ctrl-C is its to answer, and stops this only through it, but a runner
/// that crashes after one is not restarted, since its stop was asked for.
fn supervise(dir: &Path, games: Option<u32>) -> Result<(), String> {
    static ASKED: AtomicBool = AtomicBool::new(false);
    ctrlc::set_handler(|| ASKED.store(true, Ordering::Relaxed))
        .map_err(|e| format!("taking Ctrl-C: {e}"))?;
    let bin = dir.join("beryl_cli");
    let finished = || files::read::<GameRecord>(&dir.join("games.jsonl")).map(|g| g.len());
    let mut crashes = 0;
    let mut before = finished()?;
    loop {
        let mut runner = Command::new(&bin);
        runner.arg("longthink-run").arg(dir);
        if let Some(n) = games {
            runner.args(["--games", &n.to_string()]);
        }
        let status = runner
            .status()
            .map_err(|e| format!("{}: {e}", bin.display()))?;
        if status.success() {
            return Ok(());
        }
        if status.code() == Some(term::STOPPED) {
            std::process::exit(term::STOPPED);
        }
        // A panic exits with 101, or aborts under the search profile; so might a fault.
        let abnormal = status.code() == Some(101)
            || status
                .signal()
                .is_some_and(|s| [4, 5, 6, 10, 11].contains(&s));
        if !abnormal {
            return Err(format!("the runner stopped: {status}"));
        }
        if ASKED.load(Ordering::Relaxed) {
            eprintln!("the runner crashed ({status}) after a stop was asked for");
            return Ok(());
        }
        let now = finished()?;
        crashes = if now > before { 1 } else { crashes + 1 };
        before = now;
        if crashes > 10 {
            return Err("the runner crashed 10 times without finishing a game".to_owned());
        }
        eprintln!("the runner crashed ({status}); restarting it");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rungs_have_short_names() {
        let names: Vec<String> = [3_000, 10_000, 1_000_000, 3_000_000, 16, 1_500]
            .map(rung_name)
            .into();
        assert_eq!(names, ["3k", "10k", "1M", "3M", "16", "1500"]);
    }

    /// A pair is one seed, the rung's agent in each seat in turn; every rung plays the same
    /// seeds.
    #[test]
    fn a_pair_plays_one_seed_from_both_seats() {
        let c = Config::default();
        let at = |rung, against| Matchup { rung, against };
        let [a, b] = [0, 1].map(|g| Spec::new(&c, at(0, Against::Opponent), g));
        assert_eq!((a.seed, a.seat, b.seat), (b.seed, 0, 1));
        assert_eq!(a.entrants[0], b.entrants[1]);
        assert_eq!(a.key, "3k-v-expert-frozen-p000-s0");
        let up = Spec::new(&c, at(2, Against::Below), 1);
        assert_eq!((up.seed, up.key.as_str()), (a.seed, "30k-v-10k-p000-s1"));
        assert_ne!(Spec::new(&c, at(0, Against::Opponent), 2).seed, a.seed);
        let omniscient = Spec::new(&c, at(1, Against::Omniscient), 0);
        assert_eq!(omniscient.key, "omniscient-10k-v-expert-frozen-p000-s0");
    }

    /// A new run is numbered after the latest, gaps or not, so plain `longthink` resumes it.
    #[test]
    fn a_new_run_becomes_the_latest() {
        let root = std::env::temp_dir().join(format!("beryl-runs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(fresh_dir(&root).unwrap(), root.join("run-001"));
        for run in ["run-001", "run-005", "other"] {
            fs::create_dir_all(root.join(run)).unwrap();
        }
        let new = fresh_dir(&root).unwrap();
        assert_eq!(new, root.join("run-006"));
        fs::create_dir_all(&new).unwrap();
        assert_eq!(latest(&root).unwrap(), Some(new));
    }

    fn git(repo: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A commit to one of the crates, or an edit not yet committed, changes its tree id
    /// and only its.
    #[test]
    fn code_ids_follow_the_crates_working_trees() {
        let repo = std::env::temp_dir().join(format!("beryl-code-ids-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        for c in CRATES {
            fs::create_dir_all(repo.join("rust/crates").join(c)).unwrap();
            fs::write(repo.join("rust/crates").join(c).join("lib.rs"), c).unwrap();
        }
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "a"]);
        let before = code_ids(&repo).expect("tree ids");
        let lib = repo.join("rust/crates/beryl_search/lib.rs");
        fs::write(&lib, "changed").unwrap();
        let edited = code_ids(&repo).unwrap();
        assert_eq!(changed(Some(&before), Some(&edited)), ["beryl_search"]);
        git(&repo, &["commit", "-q", "-am", "b"]);
        assert_eq!(code_ids(&repo).unwrap(), edited);
        assert_eq!(changed(Some(&edited), Some(&edited)), Vec::<&str>::new());
        assert_eq!(changed(None, Some(&edited)), CRATES);
        assert!(code_ids(&std::env::temp_dir().join("beryl-no-such-repo")).is_none());
    }
}
