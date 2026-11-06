//! The long-think arena (docs/ai-strategy.md §5): an open-ended tournament of search
//! families at budgets that grow while they climb, every entrant in one rating pool, the
//! next game always the one that teaches the most per engine step ([`schedule`]), which any
//! stop costs at most the decision being searched.
//!
//! `beryl_cli longthink` (`just longthink`) starts the arena in a new run directory under
//! `$BERYL_CACHE/longthink`, or resumes the latest, and supervises the runner: the binary
//! the run was started with, copied into its directory, so later commits never change a
//! run's agents. A runner that crashes is restarted and resumes. `--games N` stops once the
//! run holds N games, however many sessions it took; `--status` prints the summary;
//! `--upgrade` adopts the current code if it plays identically ([`upgrade`]); `--add FAMILY`
//! adds a family to a run; `--new` and `--run DIR` start a new run, or resume any. Its
//! files:
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
mod ratings;
mod run;
mod schedule;
mod summary;
mod term;
mod upgrade;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use beryl_search::{AgentDecision, Family, Settings};
use serde::{Deserialize, Serialize};

use game::{Entrant, GameDecision};
pub use run::run;
use schedule::{Config, Matchup, budget_of};

/// The crates whose code decides how a run plays.
const CRATES: [&str; 3] = ["beryl_engine", "beryl_cards", "beryl_search"];

/// How a finished game went for its first side.
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

impl Verdict {
    /// How it went for the other side.
    pub fn flipped(self) -> Verdict {
        match self {
            Verdict::Win => Verdict::Loss,
            Verdict::Loss => Verdict::Win,
            other => other,
        }
    }
}

/// A line of `games.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameRecord {
    pub game: String,
    /// The matchup's sides, by name.
    pub a: String,
    pub b: String,
    pub pair: u32,
    pub seed: u64,
    /// The seat of `a`.
    pub seat: u8,
    /// How it went for `a`.
    pub outcome: Verdict,
    pub turns: u16,
    pub decisions: u32,
    /// For `a` and `b`: decisions searched, the iterations they ran and the engine steps
    /// they spent.
    pub searched: [u32; 2],
    pub iterations: [u64; 2],
    pub work: [u64; 2],
    /// The decision a crashed game crashed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crashed_at: Option<GameDecision>,
}

impl GameRecord {
    pub fn matchup(&self) -> Matchup {
        Matchup {
            a: self.a.clone(),
            b: self.b.clone(),
        }
    }
}

/// A line of `decisions.jsonl`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub game: String,
    pub decision: GameDecision,
    pub seat: u8,
    /// The entrant, by name.
    pub agent: String,
    /// Its budget, in engine steps.
    pub budget: u64,
    pub choice: u8,
    /// The decision's index among the agent's own, and that of the decision whose search
    /// planted the trees it grew.
    pub agent_decision: AgentDecision,
    pub chain: AgentDecision,
    pub iterations: u32,
    pub work: u64,
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

/// A budget, short: 300k, 3M, 1G.
pub fn rung_name(n: u64) -> String {
    match n {
        0 => "0".to_owned(),
        n if n % 1_000_000_000 == 0 => format!("{}G", n / 1_000_000_000),
        n if n % 1_000_000 == 0 => format!("{}M", n / 1_000_000),
        n if n % 1_000 == 0 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

/// Entrant `name` as a run with `c` plays it.
pub fn entrant(c: &Config, name: &str) -> Result<Entrant, String> {
    match name.rsplit_once('@') {
        Some((family, _)) => Ok(Entrant::Search(Settings {
            family: family.parse::<Family>()?,
            work: budget_of(name).ok_or_else(|| format!("{name:?}: no budget"))?,
            threads: c.threads,
        })),
        None if beryl_search::agent(name).is_some() => Ok(Entrant::Fixed(name.to_owned())),
        None => Err(format!("no agent is named {name:?}")),
    }
}

/// Game `g` of a matchup: its key, seed and agents.
#[derive(Clone, Debug)]
pub struct Spec {
    pub key: String,
    pub matchup: Matchup,
    pub pair: u32,
    pub seed: u64,
    /// The seat of the matchup's first side.
    pub seat: u8,
    /// The agents, by seat.
    pub entrants: [Entrant; 2],
}

impl Spec {
    /// Game `g` of matchup `m`: pair `g / 2`, its first side in seat `g % 2`.
    pub fn new(c: &Config, m: &Matchup, g: u32) -> Result<Spec, String> {
        let (a, b) = (entrant(c, &m.a)?, entrant(c, &m.b)?);
        let (pair, seat) = (g / 2, (g % 2) as u8);
        Ok(Spec {
            key: format!("{}-p{pair:04}-s{seat}", m.key()),
            matchup: m.clone(),
            pair,
            seed: crate::tournament::game_seed(c.seed, 0, pair),
            seat,
            entrants: if seat == 0 { [a, b] } else { [b, a] },
        })
    }

    /// The seat of side `side`: 0 for the matchup's first, 1 for its second.
    pub fn seat_of(&self, side: usize) -> usize {
        usize::from(self.seat) ^ side
    }
}

#[cfg(test)]
impl GameRecord {
    /// Game `g` of matchup `m`, ended `outcome` for its first side.
    pub fn example(m: &Matchup, g: u32, outcome: Verdict) -> GameRecord {
        let work = |e: &str| budget_of(e).map_or(0, |b| b * 60);
        GameRecord {
            game: format!("{}-{g}", m.key()),
            a: m.a.clone(),
            b: m.b.clone(),
            pair: g / 2,
            seed: 0,
            seat: (g % 2) as u8,
            outcome,
            turns: 20,
            decisions: 300,
            searched: [60, 60],
            iterations: [600, 600],
            work: [work(&m.a), work(&m.b)],
            crashed_at: None,
        }
    }
}

const USAGE: &str =
    "usage: beryl_cli longthink [--status | --upgrade | --add FAMILY] [--new | --run DIR]
      [--games N]
  a new run also takes: [--families F,F,...] [--fixed NAME,NAME,...] [--anchor NAME]
      [--budgets N,N,...] [--threads K] [--seed N]";

/// `beryl_cli longthink`: starts the arena or resumes it, and supervises it.
pub fn main(args: &[String]) -> Result<(), String> {
    let mut new = false;
    let mut status = false;
    let mut upgrade = false;
    let mut add: Vec<String> = Vec::new();
    let mut at: Option<PathBuf> = None;
    let mut games: Option<u32> = None;
    let mut c = Config::default();
    let mut configured = false;
    let list = |v: String| -> Vec<String> { v.split(',').map(str::to_owned).collect() };
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
            "--add" => add.push(value()?),
            "--run" => at = Some(PathBuf::from(value()?)),
            "--games" => games = Some(crate::number(arg, &value()?)?),
            flag => {
                configured = true;
                match flag {
                    "--families" => c.families = list(value()?),
                    "--fixed" => c.fixed = list(value()?),
                    "--anchor" => c.anchor = value()?,
                    "--budgets" => {
                        c.budgets = value()?
                            .split(',')
                            .map(|n| schedule::parse_work(n).ok_or(format!("{arg}: {n:?}")))
                            .collect::<Result<_, _>>()?;
                    }
                    "--threads" => c.threads = crate::number(arg, &value()?)?,
                    "--seed" => c.seed = crate::number(arg, &value()?)?,
                    _ => return Err(format!("unknown argument {flag}\n{USAGE}")),
                }
            }
        }
    }
    check(&c)?;
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
    if !add.is_empty() {
        add_families(&dir, &add)?;
    }
    if upgrade {
        return upgrade::adopt(&dir, code.as_ref());
    }
    supervise(&dir, games)
}

/// Whether `c` names agents and families that exist, and positive budgets and threads.
fn check(c: &Config) -> Result<(), String> {
    if c.budgets.is_empty() || c.budgets.contains(&0) || c.threads == 0 {
        return Err("budgets and threads must be positive".to_owned());
    }
    for f in &c.families {
        f.parse::<Family>()?;
    }
    for name in &c.fixed {
        if beryl_search::agent(name).is_none() {
            return Err(format!("no agent is named {name:?}"));
        }
    }
    if !c.fixed.contains(&c.anchor) {
        return Err(format!("the anchor {:?} is not a fixed agent", c.anchor));
    }
    Ok(())
}

/// Adds `families` to run `dir`'s settings, those it lacks: the arena opens them at once.
fn add_families(dir: &Path, families: &[String]) -> Result<(), String> {
    let mut c = config(dir)?;
    for f in families {
        let parsed: Family = f.parse()?;
        let name = parsed.to_string();
        if c.families.contains(&name) {
            println!("{name} already plays");
        } else {
            println!("added {name}");
            c.families.push(name);
        }
    }
    let json = serde_json::to_string_pretty(&c).map_err(|e| e.to_string())? + "\n";
    files::write_whole(&dir.join("config.json"), json.as_bytes())
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

    /// A pair is one seed, each side in each seat in turn; every matchup plays the same
    /// seeds.
    #[test]
    fn a_pair_plays_one_seed_from_both_seats() {
        let c = Config::default();
        let m = Matchup::new("expert-frozen", "ismcts18/hidden/rollout/17l@1M");
        let [a, b] = [0, 1].map(|g| Spec::new(&c, &m, g).unwrap());
        assert_eq!((a.seed, a.seat, b.seat), (b.seed, 0, 1));
        assert_eq!(a.entrants[0], b.entrants[1]);
        assert_eq!(
            a.key,
            "expert-frozen~ismcts18.hidden.rollout.17l@1M-p0000-s0"
        );
        let other = Spec::new(&c, &Matchup::new("greedy", "expert-frozen"), 1).unwrap();
        assert_eq!(other.seed, a.seed);
        assert_ne!(Spec::new(&c, &m, 2).unwrap().seed, a.seed);
        assert_eq!(b.seat_of(0), 1);
        assert!(entrant(&c, "nobody").is_err() && entrant(&c, "pimc0/x@1M").is_err());
    }

    #[test]
    fn rungs_have_short_names() {
        let names: Vec<String> = [3_000, 10_000, 1_000_000, 3_000_000, 2_000_000_000, 1_500]
            .map(rung_name)
            .into();
        assert_eq!(names, ["3k", "10k", "1M", "3M", "2G", "1500"]);
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
