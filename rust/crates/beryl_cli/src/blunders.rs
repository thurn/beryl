//! `beryl_cli blunders [agent] [--work 3M,30M] [--seeds 5] [--only NAME,...] [--json FILE]`
//! (`just blunders`): the blunder suite (docs/ai-strategy.md §5), a regression gate built
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
use beryl_search::{Agent, Budget, DEEP_SEARCHER};
use beryl_session::replay::Entry as LogEntry;
use beryl_session::{ReplayFile, StopsSetting};
use serde::Serialize;

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
fn root() -> PathBuf {
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
pub fn reach(e: &Entry) -> Result<Reached, String> {
    let drift = |why: String| format!("{} ({}) drifted: {why}", e.name, e.incident);
    let mut walk = match &e.source {
        Source::Replay(path) => {
            let path = root().join(path);
            let file = ReplayFile::load(&path).map_err(|err| drift(format!("{err:?}")))?;
            Walk::of_file(&file).map_err(drift)?
        }
        Source::Seeded(path) => {
            let path = root().join(path);
            let text = fs::read_to_string(&path).map_err(|err| drift(err.to_string()))?;
            let r: Seeded = serde_json::from_str(&text).map_err(|err| drift(err.to_string()))?;
            let t = mirror();
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
            let t = mirror();
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
    loop {
        let shown = labels(t, view(&s, t, &mut x), &d);
        let c = agent.choose(observe(&s, t, &mut x, d.player), &d);
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
#[derive(Clone, Debug, Serialize)]
pub struct Run {
    pub budget: String,
    pub seed: u32,
    pub chosen: Vec<String>,
    pub pass: bool,
}

/// An entry's state for the agent run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    Fixed,
    Open,
    Drifted,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntryReport {
    pub name: String,
    pub incident: String,
    pub state: Standing,
    pub runs: Vec<Run>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
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
pub(crate) fn run_seed(name: &str, k: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    SplitMix64::new(h ^ u64::from(k)).next_u64()
}

/// The agent `name` for a run from `seed`: a registered agent, or a long-think entrant
/// searching on every core.
fn build(t: &GameTable, name: &str, seed: u64) -> Result<Box<dyn Agent>, String> {
    if let Some(info) = beryl_search::agent(name) {
        return Ok((info.new)(t, seed, Budget::DEFAULT));
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
    let built =
        game::entrant(t, name, seed, threads).ok_or_else(|| format!("no agent {name:?}"))??;
    Ok(built)
}

/// Runs the suite's `entries` for `agent` (a family, played at each of `budgets`, or a
/// registered agent or `family@budget`, played as named), from `seeds` seeds each.
pub fn run(
    entries: &[Entry],
    agent: &str,
    budgets: &[u64],
    seeds: u32,
    mut progress: impl FnMut(&str),
) -> Result<Report, String> {
    let names: Vec<(String, String)> =
        if agent.contains('@') || beryl_search::agent(agent).is_some() {
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
        match reach(e) {
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
                if r.runs.iter().all(|run| run.pass) {
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

pub fn main(args: &[String]) -> Result<(), String> {
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
    let report = run(&entries, &agent, &budgets, seeds, |line| {
        eprintln!("{line}");
    })?;
    print!("{}", table(&report));
    if let Some(path) = json {
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n";
        fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
    }
    if report.drifted > 0 {
        return Err(format!(
            "{} entries drifted: rebuild or retire them (docs/ai-strategy.md §5)",
            report.drifted
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An agent that plays the first candidate whose label it lists, else the first.
    struct Scripted {
        t: GameTable,
        picks: Vec<&'static str>,
    }

    impl Agent for Scripted {
        fn choose(
            &mut self,
            obs: beryl_engine::Observation<'_>,
            d: &Decision,
        ) -> beryl_engine::Choice {
            let mut x = Scratch::new();
            let root = obs.unmasked_root();
            let shown = labels(&self.t, view(root, &self.t, &mut x), d);
            let i = shown
                .iter()
                .position(|l| self.picks.contains(&l.as_str()))
                .unwrap_or(0);
            beryl_engine::Choice(u16::try_from(i).unwrap())
        }
    }

    fn entry(name: &str) -> Entry {
        entries()
            .unwrap()
            .into_iter()
            .find(|e| e.name == name)
            .unwrap()
    }

    fn scripted(e: &Entry, picks: &[&'static str]) -> bool {
        let at = reach(e).unwrap();
        let mut agent = Scripted {
            t: mirror(),
            picks: picks.to_vec(),
        };
        passes(e, &play(e, &at, &mut agent))
    }

    /// Every entry parses and its record reaches its position, where the game played what
    /// the incident says.
    #[test]
    fn every_entry_reaches_its_position() {
        let all = entries().unwrap();
        assert!(all.len() >= 7);
        for e in &all {
            let at = reach(e).unwrap_or_else(|why| panic!("{why}"));
            assert_eq!(at.d.player.get(), e.seat, "{}", e.name);
            assert!(
                root()
                    .join("docs/incidents")
                    .join(format!("{}.md", e.incident))
                    .exists()
            );
            assert!(
                !e.forbidden.is_empty() || !e.required.is_empty(),
                "{}",
                e.name
            );
        }
    }

    /// A forbidden choice fails its entry and another passes; a required choice passes and a
    /// missed one fails, over the several decisions of an attack too.
    #[test]
    fn scripted_choices_pass_and_fail() {
        let oust = entry("oust-own-token");
        assert!(!scripted(&oust, &["cast Oust"]));
        assert!(scripted(&oust, &["pass"]));
        let crypt = entry("decline-mana-crypt");
        assert!(scripted(&crypt, &["yes", "Mana Crypt"]));
        assert!(!scripted(&crypt, &["yes", "done"]));
        assert!(!scripted(&crypt, &["no"]));
        let jace = entry("spare-jace-wielder");
        assert!(scripted(
            &jace,
            &[
                "attack with Snapcaster Mage",
                "attack with Spirit Token",
                "Jace, Wielder of Mysteries"
            ]
        ));
        assert!(!scripted(
            &jace,
            &["attack with Snapcaster Mage", "Jace, the Mind Sculptor"]
        ));
    }

    #[test]
    fn combat_planner_holds_back_at_the_two_blocker_positions() {
        for name in ["beza-trinket-into-blockers", "manifest-into-samurai"] {
            let at = reach(&entry(name)).unwrap();
            let seed = run_seed(name, 0);
            let family = "pimc2/hidden/rollout/tuned/anchor+combat".parse().unwrap();
            let mut scratch = Scratch::new();
            let obs = observe(&at.s, &at.t, &mut scratch, at.d.player);
            let shown = labels(&at.t, view(&at.s, &at.t, &mut Scratch::new()), &at.d);
            let settings = |work| beryl_search::Settings {
                family,
                work,
                threads: 1,
            };

            // With no budget for an iteration, the guarded decision uses the default.
            let mut planner = beryl_search::LongSearch::new(&at.t, settings(0), seed);
            let choice = planner.choose(obs, &at.d);
            assert_eq!(planner.last_search().unwrap().rollouts, 0);
            assert_eq!(shown[usize::from(choice.0)], "done", "{name}");

            let mut first = beryl_search::LongSearch::new(&at.t, settings(20_000), seed);
            let mut second = beryl_search::LongSearch::new(&at.t, settings(20_000), seed);
            assert_eq!(
                first.choose(obs, &at.d),
                second.choose(obs, &at.d),
                "{name}"
            );
        }
    }

    /// Reviewed hunt positions reject the recorded mistake but retain a useful alternative.
    #[test]
    fn reviewed_hunt_choices_and_drift() {
        for (name, bad, good) in [
            (
                "discard-emperor-over-spare-crypt",
                "The Wandering Emperor",
                "Mana Crypt",
            ),
            ("decline-second-teferi-untap", "done", "Island"),
            (
                "evoke-subtlety-empty-stack-hunt",
                "cast Subtlety for evoke",
                "pass",
            ),
        ] {
            let mut e = entry(name);
            assert!(!scripted(&e, &[bad]), "{name}");
            assert!(scripted(&e, &[good]), "{name}");
            e.played = "not the recorded choice".to_owned();
            let why = reach(&e).err().expect("changed choice must report drift");
            assert!(why.contains(name), "{why}");
            assert!(why.contains(&e.incident), "{why}");
        }
    }

    /// A record that no longer plays what the incident says is reported with its incident.
    #[test]
    fn a_drifted_replay_names_its_incident() {
        let mut e = entry("evoke-subtlety-no-target");
        e.played = "pass".to_owned();
        let why = reach(&e).err().unwrap();
        assert!(why.contains("2026-11-08-subtlety-spirits-samurai"), "{why}");
        let mut e = entry("oust-own-token");
        e.turn = 13;
        assert!(reach(&e).is_err());
    }

    #[test]
    fn runs_are_judged_by_their_choices() {
        let e = entry("spare-jace-wielder");
        let both = [
            "attack with Spirit Token",
            "Jace, Wielder of Mysteries",
            "attack with Snapcaster Mage",
            "Jace, Wielder of Mysteries",
        ]
        .map(str::to_owned);
        assert!(passes(&e, &both));
        assert!(!passes(&e, &both[..3]));
        assert_ne!(run_seed("a", 0), run_seed("b", 0));
        assert_ne!(run_seed("a", 0), run_seed("a", 1));
    }
}
