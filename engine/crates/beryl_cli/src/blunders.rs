//! `beryl_cli blunders [agent] [--work 3M,30M] [--seeds 5] [--only NAME,...] [--json FILE]`
//! (`just blunders`): the blunder suite, a regression gate built
//! from every blunder a human has caught.
//!
//! - **An entry** of `data/blunders.json` ([`Entry`]) names an incident, a position and the
//!   choices forbidden or required there, labeled as `just review` labels the decision's
//!   candidates. The position is a decision of an incident's replay (its turn and its index
//!   among that turn's decisions), or of a rebuilt board ([`Board`]) where the replay no
//!   longer reproduces its game. A blunder spread over several decisions, as an attack is,
//!   names the decision kinds the agent goes on through ([`Entry::through`]), and its choices
//!   are judged together.
//! - **The check** replays each entry to its position, builds the agent (by default the
//!   champion, [`DEEP_SEARCHER`]) at each budget from each seed, lets it play the decision,
//!   and records its choices. A run fails on any forbidden choice or any required one missed.
//! - **Open and fixed.** An entry is fixed for the agent when every run passes, open
//!   otherwise. The suite is a ledger: the champion made every play it forbids, so some
//!   entries are open at any time. A rung's gate compares two runs' `--json`: it fixes at
//!   least one open entry it claimed and re-opens no fixed one.
//! - **Drift.** An entry whose record no longer reaches its position, or whose game played
//!   there other than the incident says ([`Entry::played`]), is reported with its incident
//!   and fails the command: never skipped. §5 says how it is rebuilt or retired.
//!
//! A rebuilt board (`data/blunders/*.json`) holds the game as the turn's first priority of its
//! active player found it, on the last build that still replayed the incident's record
//! (cae2787d), and the record's answers from there by what they do. It keeps the board, the
//! hands, the graveyards, exile and each library's order; what a player knew of a hidden card
//! other than a revealed hand card is not kept.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Decision, GameTable, Profile, Replay, Scratch, State, Step, apply, next, observe, view,
};
use beryl_search::{Agent, AgentDecision, Budget, DEEP_SEARCHER};
use beryl_session::replay::Entry as LogEntry;
use beryl_session::{ReplayFile, StopsSetting};
use serde::{Deserialize, Serialize};

use crate::Catalog;
use crate::game::{self, mirror, parse_work};
use crate::ladder::resolve;
use crate::match_log::rung_name;
use crate::transcript::labels;
use crate::walk::{Walk, Walked};

mod board;
mod entries;

pub use board::arrange;
use board::seat;
use entries::Seeded;
pub use entries::{Entry, Rebuilt, Source, entries};

const USAGE: &str = "usage: beryl_cli blunders [agent] [--work 3M,30M] [--seeds 5] \
                     [--only NAME,...] [--json FILE]";

pub const BUDGETS: [u64; 2] = [3_000_000, 30_000_000];
pub const SEEDS: u32 = 5;

/// The repository, whose `data/` and `docs/` the entries name files in: above this crate
/// when run under cargo, else the nearest directory up from here that holds the suite.
pub fn root() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR") {
        return Path::new(&dir).join("../../..");
    }
    let here = std::env::current_dir().unwrap_or_default();
    here.ancestors()
        .find(|d| d.join("data/blunders.json").exists())
        .map_or(here.clone(), Path::to_path_buf)
}

/// An entry's position, reached: the game and the decision open there.
pub struct Reached {
    pub t: GameTable,
    pub s: State,
    pub d: Decision,
}

/// Replays `e`'s record to its position, failing, with its incident named, where it drifted.
pub fn reach(catalog: &'static Catalog, e: &Entry) -> Result<Reached, String> {
    let drift = |why: String| format!("{} ({}) drifted: {why}", e.name, e.incident);
    let mut walk = match &e.source {
        Source::Replay(path) => {
            let path = root().join(path);
            let file =
                ReplayFile::load(&path, catalog.cards).map_err(|err| drift(format!("{err:?}")))?;
            Walk::of_file(catalog, &file).map_err(drift)?
        }
        Source::Seeded(path) => {
            let path = root().join(path);
            let text = fs::read_to_string(&path).map_err(|err| drift(err.to_string()))?;
            let r: Seeded = serde_json::from_str(&text).map_err(|err| drift(err.to_string()))?;
            let t = mirror(catalog);
            let s = State::new(&t, r.seed, Profile::SMART).map_err(|e| drift(e.to_string()))?;
            let log = r.history.into_iter().map(LogEntry::Choice).collect();
            // Hunt games have no human stops or automatic session answers. Do not use
            // Walk::of_file, which changes one seat's profile to the app's human profile.
            Walk::new(t, &s, log, StopsSetting::default(), seat(e.seat)?)
        }
        Source::Board(path) => {
            let path = root().join(path);
            let text =
                fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            let r: Rebuilt =
                serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
            let t = mirror(catalog);
            let s = arrange(&t, &r.board).map_err(drift)?;
            Walk::new(t, &s, r.path, r.board.stops, seat(r.board.human)?)
        }
    };
    let mut index = 0;
    loop {
        match walk.next().map_err(drift)? {
            Walked::Decide { d, c, .. } => {
                let turn = walk.s.turn().number();
                if turn > e.turn {
                    return Err(drift(format!(
                        "turn {} has no decision {}",
                        e.turn, e.index
                    )));
                }
                if turn == e.turn {
                    if index == e.index {
                        let shown = labels(&walk.t, view(&walk.s, &walk.t, &mut walk.x), &d);
                        let played = &shown[usize::from(c.0)];
                        if d.player.get() != e.seat || *played != e.played {
                            return Err(drift(format!(
                                "player {} played {played:?} there, not player {} {:?} (of {})",
                                d.player.get(),
                                e.seat,
                                e.played,
                                shown.join(" | ")
                            )));
                        }
                        return Ok(Reached {
                            t: walk.t,
                            s: walk.s,
                            d,
                        });
                    }
                    index += 1;
                }
                walk.apply(c);
            }
            Walked::Undone => {}
            Walked::Conceded | Walked::Ends | Walked::Over { .. } => {
                return Err(drift("the record ends first".to_owned()));
            }
        }
    }
}

/// The labels `agent` chooses at the position and on through the decisions `e` names.
pub fn play(e: &Entry, at: &Reached, agent: &mut dyn Agent) -> Vec<String> {
    let (t, mut s, mut d) = (&at.t, at.s, at.d);
    let mut x = Scratch::new();
    let mut chosen = Vec::new();
    // The agent's own decisions, counted from the position: it is asked every one from there.
    let mut own = AgentDecision(0);
    loop {
        let shown = labels(t, view(&s, t, &mut x), &d);
        let c = agent.choose(observe(&s, t, &mut x, d.decider), &d, own.take());
        chosen.push(shown[usize::from(c.0)].clone());
        apply(&mut s, t, &mut x, &mut Replay, c);
        match next(&mut s, t, &mut x, &mut Replay) {
            Step::Decide(n)
                if n.player.get() == e.seat && e.through.contains(&format!("{:?}", n.kind)) =>
            {
                d = n;
            }
            _ => return chosen,
        }
    }
}

/// Whether `chosen` avoids every forbidden label and holds every required one.
pub fn passes(e: &Entry, chosen: &[String]) -> bool {
    if chosen.iter().any(|c| e.forbidden.contains(c)) {
        return false;
    }
    let mut left: Vec<&String> = chosen.iter().collect();
    e.required
        .iter()
        .all(|r| match left.iter().position(|c| *c == r) {
            Some(i) => {
                left.swap_remove(i);
                true
            }
            None => false,
        })
}

/// One run: an agent's choices at an entry, from one seed at one budget.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub budget: String,
    pub seed: u32,
    pub chosen: Vec<String>,
    pub pass: bool,
}

/// An entry's state for the agent run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    Fixed,
    Open,
    Drifted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryReport {
    pub name: String,
    pub incident: String,
    pub state: Standing,
    pub runs: Vec<Run>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub agent: String,
    pub budgets: Vec<String>,
    pub seeds: u32,
    pub open: u32,
    pub fixed: u32,
    pub drifted: u32,
    pub entries: Vec<EntryReport>,
}

/// The seed of run `k` at entry `name`: its own stream per entry, so adding an entry moves
/// no other's.
pub fn run_seed(name: &str, k: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    SplitMix64::new(h ^ u64::from(k)).next_u64()
}

/// The agent `name` for a run from `seed`: a registered agent, or a long-think entrant
/// searching on every core.
pub fn build(t: &GameTable, name: &str, seed: u64) -> Result<Box<dyn Agent>, String> {
    if let Some(info) = beryl_search::find_agent(name)? {
        return Ok((info.new)(t, seed, Budget::DEFAULT));
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
    let built =
        game::entrant(t, name, seed, threads).ok_or_else(|| format!("no agent {name:?}"))??;
    Ok(built)
}

/// Runs the suite's `entries` for `agent` (a family, played at each of `budgets`, or a
/// registered agent or `family@budget`, played as named), from seed 0 through `seeds - 1`
/// at each budget. An entry is fixed if it misses at most `allowed_misses` runs.
pub fn run(
    catalog: &'static Catalog,
    entries: &[Entry],
    agent: &str,
    budgets: &[u64],
    seeds: u32,
    allowed_misses: usize,
    progress: impl FnMut(&str) + Send,
) -> Result<Report, String> {
    crate::workers::run(move || {
        run_suite(
            catalog,
            entries,
            agent,
            budgets,
            seeds,
            allowed_misses,
            progress,
        )
    })
}

fn run_suite(
    catalog: &'static Catalog,
    entries: &[Entry],
    agent: &str,
    budgets: &[u64],
    seeds: u32,
    allowed_misses: usize,
    mut progress: impl FnMut(&str),
) -> Result<Report, String> {
    let names: Vec<(String, String)> =
        if beryl_search::find_agent(agent)?.is_some() || agent.contains('@') {
            vec![(agent.to_owned(), "as named".to_owned())]
        } else {
            budgets
                .iter()
                .map(|&w| Ok((resolve(agent, w)?, rung_name(w))))
                .collect::<Result<_, String>>()?
        };
    let mut report = Report {
        agent: agent.to_owned(),
        budgets: names.iter().map(|(_, b)| b.clone()).collect(),
        seeds,
        open: 0,
        fixed: 0,
        drifted: 0,
        entries: Vec::new(),
    };
    for e in entries {
        let mut r = EntryReport {
            name: e.name.clone(),
            incident: e.incident.clone(),
            state: Standing::Fixed,
            runs: Vec::new(),
            drift: None,
        };
        match reach(catalog, e) {
            Err(why) => {
                r.state = Standing::Drifted;
                r.drift = Some(why);
                report.drifted += 1;
            }
            Ok(at) => {
                for (name, budget) in &names {
                    for k in 0..seeds {
                        let mut agent = build(&at.t, name, run_seed(&e.name, k))?;
                        let chosen = play(e, &at, agent.as_mut());
                        let pass = passes(e, &chosen);
                        progress(&format!(
                            "{} {budget} seed {k}: {} {}",
                            e.name,
                            chosen.join(", "),
                            if pass { "pass" } else { "FAIL" }
                        ));
                        r.runs.push(Run {
                            budget: budget.clone(),
                            seed: k,
                            chosen,
                            pass,
                        });
                    }
                }
                if r.runs.iter().filter(|run| !run.pass).count() <= allowed_misses {
                    report.fixed += 1;
                } else {
                    r.state = Standing::Open;
                    report.open += 1;
                }
            }
        }
        report.entries.push(r);
    }
    Ok(report)
}

/// The report as a table: each entry's runs by budget, passes and the choices made.
pub fn table(r: &Report) -> String {
    let mut out = format!(
        "{} from {} seeds: {} fixed, {} open, {} drifted\n\n{:<26} {:<8} {:<9} {:>5}  choices\n",
        r.agent, r.seeds, r.fixed, r.open, r.drifted, "entry", "state", "budget", "pass"
    );
    for e in &r.entries {
        let state = match e.state {
            Standing::Fixed => "fixed",
            Standing::Open => "open",
            Standing::Drifted => "DRIFTED",
        };
        if let Some(why) = &e.drift {
            let _ = writeln!(out, "{:<26} {state:<8} {why}", e.name);
            continue;
        }
        for budget in &r.budgets {
            let runs: Vec<&Run> = e.runs.iter().filter(|run| &run.budget == budget).collect();
            let passed = runs.iter().filter(|run| run.pass).count();
            let mut counts: BTreeMap<String, u32> = BTreeMap::new();
            for run in &runs {
                *counts.entry(run.chosen.join(" + ")).or_default() += 1;
            }
            let choices: Vec<String> = counts.iter().map(|(c, n)| format!("{c} x{n}")).collect();
            let _ = writeln!(
                out,
                "{:<26} {state:<8} {budget:<9} {passed:>2}/{:<2}  {}",
                e.name,
                runs.len(),
                choices.join("; ")
            );
        }
    }
    out
}

pub fn main(catalog: &'static Catalog, args: &[String]) -> Result<(), String> {
    let mut agent = None;
    let mut budgets = BUDGETS.to_vec();
    let mut seeds = SEEDS;
    let mut only: Option<Vec<String>> = None;
    let mut json = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--work" => {
                budgets = value()?
                    .split(',')
                    .map(|w| parse_work(w).ok_or_else(|| format!("--work: no budget {w:?}")))
                    .collect::<Result<_, _>>()?;
            }
            "--seeds" => seeds = crate::number(arg, &value()?)?,
            "--only" => only = Some(value()?.split(',').map(str::to_owned).collect()),
            "--json" => json = Some(value()?),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}\n{USAGE}")),
            name if agent.is_none() => agent = Some(name.to_owned()),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let agent = agent.unwrap_or_else(|| DEEP_SEARCHER.to_string());
    let mut entries = entries()?;
    if let Some(only) = &only {
        if let Some(missing) = only.iter().find(|n| !entries.iter().any(|e| &e.name == *n)) {
            return Err(format!("no entry {missing:?}"));
        }
        entries.retain(|e| only.contains(&e.name));
    }
    let report = run(catalog, &entries, &agent, &budgets, seeds, 0, |line| {
        eprintln!("{line}");
    })?;
    print!("{}", table(&report));
    if let Some(path) = json {
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
        fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
    }
    if report.drifted > 0 {
        return Err(format!(
            "{} entries drifted: rebuild each as a board in data/blunders/ or retire it",
            report.drifted
        ));
    }
    Ok(())
}
