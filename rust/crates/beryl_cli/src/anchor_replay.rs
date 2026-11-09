//! `beryl_cli anchor-replay <run-dir> <entrant> [--examples N]`: what anchoring a long-think
//! entrant to greedy's answer (`beryl_search::Options::anchor`, epic by-7wl) would have
//! changed in a run's games, from their logs alone, searching nothing. Each game replays from
//! its seed through its logged answers; at each of the entrant's searched decisions, greedy's
//! answer is set beside the search's root visits and means. For each margin it reports how
//! often the anchor would have played greedy's answer instead and what the search itself
//! estimates that costs: the sum, over those decisions, of its choice's mean less greedy's.
//!
//! The logs keep each candidate's visits and rewards summed over the trees, not tree by tree,
//! so the noise term here is a binomial standard error from the visits: an approximation of
//! the anchor's own, which reads the spread across the trees.
//!
//! That estimate of the cost is biased upward: the search's choice is the candidate it
//! visited most, which leads greedy's answer even where the two are worth the same. With
//! `--recheck N`, N of the decisions switched are searched again from a fresh seed at the
//! entrant's budget, the whole of it split between the two candidates
//! (`LongSearch::duel`), and the cost is estimated again from those leads, which carry no
//! such bias.
//!
//! With `--guarded`, only the decisions an anchored search guards (`beryl_search::guarded`)
//! count; without it, every decision where greedy's answer differs.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Profile, Replay, Scratch, State, Step, apply, next,
    observe,
};
use beryl_search::{ANCHOR_ERRORS, Family, LongSearch, RolloutPolicy, Settings, guarded};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::game::{mirror, parse_work};
use crate::positions::{self, Position};
use crate::transcript::Transcript;

/// The margins reported, in mean reward.
const MARGINS: [f64; 5] = [0.0, 0.005, 0.01, 0.02, 0.04];

/// The most the anchor may cost a game, by the search's own estimate, at the margin
/// recommended.
const COST_MAX: f64 = 0.01;

/// The fields of a `games.jsonl` line this reads.
#[derive(Clone, Debug, Deserialize)]
pub struct GameLine {
    pub game: String,
    pub a: String,
    pub b: String,
    pub seed: u64,
    /// The seat of `a`.
    pub seat: u8,
    pub outcome: String,
    /// The ladder position it started from, under the run's `positions/`.
    #[serde(default)]
    pub position: Option<String>,
}

impl GameLine {
    /// The seat `entrant` played, if it played this game.
    pub fn seat_of(&self, entrant: &str) -> Option<u8> {
        if self.a == entrant {
            Some(self.seat)
        } else if self.b == entrant {
            Some(1 - self.seat)
        } else {
            None
        }
    }
}

/// The finished games of the run or match in `dir`, crashed ones left out.
pub fn games(dir: &Path) -> Result<Vec<GameLine>, String> {
    let path = dir.join("games.jsonl");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut games = Vec::new();
    for g in text.lines().map(serde_json::from_str::<GameLine>) {
        let g = g.map_err(|e| format!("{}: {e}", path.display()))?;
        if g.outcome != "crashed" {
            games.push(g);
        }
    }
    Ok(games)
}

/// The state game `g` of the run in `dir` started from: its seed's shuffle, or its ladder
/// position.
pub fn start(dir: &Path, t: &GameTable, g: &GameLine) -> Result<State, String> {
    match &g.position {
        Some(name) => {
            let path = dir.join("positions").join(name);
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let p: Position =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            positions::start(t, &p, g.seed)
        }
        None => Ok(State::new(t, g.seed, Profile::SMART)),
    }
}

/// The lines of game `g`'s log in `dir`, as `T` reads them.
pub fn log<T: DeserializeOwned>(dir: &Path, g: &GameLine) -> Result<Vec<T>, String> {
    let path = dir.join("logs").join(format!("{}.jsonl", g.game));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .map(|l| serde_json::from_str(l).map_err(|e| format!("{}: {e}", path.display())))
        .collect()
}

/// A fresh duel of candidates `a` and `b` at decision `d` of `s`, the whole budget of
/// `settings` split between them (`LongSearch::duel`), from `seed`: `a`'s lead over `b`, tree
/// by tree, without the bias of a lead the search chose.
pub fn duel(
    t: &GameTable,
    settings: Settings,
    seed: u64,
    s: &State,
    d: &Decision,
    a: Choice,
    b: Choice,
) -> beryl_search::AnchorCheck {
    let mut x = Scratch::new();
    let search = LongSearch::new(t, settings, seed);
    search.duel(observe(s, t, &mut x, d.player), d, a, b)
}

/// The fields of a game log's line this reads.
#[derive(Deserialize)]
struct LogLine {
    seat: u8,
    choice: u16,
    search: Option<Searched>,
}

/// A logged search's root: each candidate's visits and summed rewards.
#[derive(Deserialize)]
pub struct Searched {
    pub visits: Vec<u32>,
    pub rewards: Vec<f64>,
}

/// One searched decision where greedy's answer differs from the search's choice.
struct Differs {
    game: String,
    state: State,
    decision: Decision,
    search: Choice,
    greedy: Choice,
    /// The search's choice's mean less greedy's, and the binomial standard error of that
    /// difference; none if the search never visited greedy's answer.
    lead: Option<(f64, f64)>,
    /// The decision as a transcript shows it, with both candidates' visits and means.
    line: String,
}

pub fn main(args: &[String]) -> Result<(), String> {
    let usage = || {
        "usage: beryl_cli anchor-replay <run-dir> <entrant> [--examples N] [--recheck N] \
         [--guarded]"
            .to_owned()
    };
    let [dir, entrant, flags @ ..] = args else {
        return Err(usage());
    };
    let (mut examples, mut recheck) = (12, 0);
    let only_guarded = flags.iter().any(|f| f == "--guarded");
    let flags: Vec<&String> = flags.iter().filter(|f| *f != "--guarded").collect();
    for pair in flags.chunks(2) {
        let n = pair.get(1).and_then(|n| n.parse().ok()).ok_or_else(usage)?;
        match pair[0].as_str() {
            "--examples" => examples = n,
            "--recheck" => recheck = n,
            _ => return Err(usage()),
        }
    }
    print!(
        "{}",
        replay(Path::new(dir), entrant, examples, recheck, only_guarded)?
    );
    Ok(())
}

fn replay(
    dir: &Path,
    entrant: &str,
    examples: usize,
    recheck: usize,
    only_guarded: bool,
) -> Result<String, String> {
    let bad = || format!("{entrant:?} is no long-think entrant (family@budget)");
    let (family, budget) = entrant.split_once('@').ok_or_else(bad)?;
    let family: Family = family.parse()?;
    let work = parse_work(budget).ok_or_else(bad)?;
    let t = mirror();
    let policy = RolloutPolicy::with(&t, family.knowledge);
    let mut searched = 0u32;
    let mut differs = Vec::new();
    let played = walk(dir, entrant, |game, s, d, choice, search, names| {
        searched += 1;
        let mut x = Scratch::new();
        let obs = observe(s, &t, &mut x, d.player);
        let greedy = policy.choose(&t, obs, d);
        let counted = !only_guarded || guarded(&policy, obs, d, greedy, Choice(choice));
        if greedy.0 != choice && counted {
            let mean = |c: u16| {
                let v = f64::from(search.visits[usize::from(c)]);
                (v > 0.0).then(|| (search.rewards[usize::from(c)] / v, v))
            };
            let lead = mean(choice)
                .zip(mean(greedy.0))
                .map(|((mc, vc), (mg, vg))| {
                    let var = |m: f64, v: f64| m * (1.0 - m) / v;
                    (mc - mg, (var(mc, vc) + var(mg, vg)).sqrt())
                });
            let mut transcript = Transcript::new(names);
            transcript.decision(&t, s, d, Choice(choice), &[]);
            let shown = transcript.text().lines().last().unwrap_or_default().trim();
            let label =
                |c: u16| mean(c).map_or("unvisited".to_owned(), |(m, v)| format!("{v} {m:.3}"));
            differs.push(Differs {
                game: game.to_owned(),
                state: *s,
                decision: *d,
                search: Choice(choice),
                greedy,
                lead,
                line: format!(
                    "{:?}: {shown}  search [{choice}] {}  greedy [{}] {}",
                    d.kind,
                    label(choice),
                    greedy.0,
                    label(greedy.0)
                ),
            });
        }
    })?;
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
    let settings = Settings {
        family,
        work,
        threads,
    };
    let fresh = rechecked(&t, settings, &differs, recheck);
    Ok(report(
        entrant, played, searched, &differs, &fresh, examples,
    ))
}

/// Replays each of the run's games `entrant` played from its seed through its log, calling
/// `visit` at each of the entrant's searched decisions with the game, the state, the decision,
/// the logged answer, the search's root visits and rewards, and the seats' names for a
/// transcript. Returns the games walked; crashed ones are skipped.
pub fn walk(
    dir: &Path,
    entrant: &str,
    mut visit: impl FnMut(&str, &State, &Decision, u16, &Searched, [&str; 2]),
) -> Result<u32, String> {
    let t = mirror();
    let mut played = 0;
    for g in games(dir)? {
        let Some(seat) = g.seat_of(entrant) else {
            continue;
        };
        played += 1;
        let lines: Vec<LogLine> = log(dir, &g)?;
        let names = if seat == 0 {
            [entrant, "opponent"]
        } else {
            ["opponent", entrant]
        };
        let mut s = start(dir, &t, &g)?;
        let mut x = Scratch::new();
        for line in lines {
            let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
                return Err(format!("{}: the game ended before its log", g.game));
            };
            if d.player.index() != usize::from(line.seat) || usize::from(line.choice) >= d.len() {
                return Err(format!("{}: the log no longer fits the game", g.game));
            }
            if let Some(search) = line.search.as_ref().filter(|_| line.seat == seat) {
                visit(&g.game, &s, &d, line.choice, search, names);
            }
            apply(&mut s, &t, &mut x, &mut Replay, Choice(line.choice));
        }
    }
    Ok(played)
}

/// Fresh leads, by index into `differs`, for `n` of the decisions the widest margin switches,
/// spread evenly over them: each the search's choice's lead over greedy's answer from a new
/// seed, the budget split between the two.
fn rechecked(
    t: &beryl_engine::GameTable,
    settings: Settings,
    differs: &[Differs],
    n: usize,
) -> Vec<(usize, f64)> {
    let widest = MARGINS[MARGINS.len() - 1];
    let switched: Vec<usize> = (0..differs.len())
        .filter(|&i| switches(&differs[i], widest))
        .collect();
    if n == 0 || switched.is_empty() {
        return Vec::new();
    }
    let step = (switched.len() as f64 / n as f64).max(1.0);
    (0..n.min(switched.len()))
        .map(|k| switched[(k as f64 * step) as usize])
        .map(|i| {
            let d = &differs[i];
            let duel = duel(
                t,
                settings,
                0x5EED_A11C ^ i as u64,
                &d.state,
                &d.decision,
                d.search,
                d.greedy,
            );
            eprintln!(
                "rechecked {}: lead {:+.4} ± {:.4}",
                i, duel.lead, duel.error
            );
            (i, duel.lead)
        })
        .collect()
}

/// Whether the anchor would play greedy's answer over the search's choice at `margin`.
fn switches(d: &Differs, margin: f64) -> bool {
    d.lead
        .is_none_or(|(lead, error)| lead <= margin.max(ANCHOR_ERRORS * error))
}

fn report(
    entrant: &str,
    played: u32,
    searched: u32,
    differs: &[Differs],
    fresh: &[(usize, f64)],
    examples: usize,
) -> String {
    let mut out = String::new();
    let games = f64::from(played.max(1));
    let share = |n: usize| 100.0 * n as f64 / f64::from(searched.max(1));
    let _ = writeln!(
        out,
        "{entrant}: {played} games, {searched} searched decisions; greedy's answer differs \
         at {} ({:.1}%), {} of them unvisited",
        differs.len(),
        share(differs.len()),
        differs.iter().filter(|d| d.lead.is_none()).count()
    );
    let _ = writeln!(
        out,
        "\nmargin  switched        a game  logged cost a game (net, gross)  fresh cost a game"
    );
    let mut recommended = None;
    for margin in MARGINS {
        let switched: Vec<&Differs> = differs.iter().filter(|d| switches(d, margin)).collect();
        let leads = switched.iter().filter_map(|d| d.lead.map(|(l, _)| l));
        let net: f64 = leads.clone().sum::<f64>() / games;
        let gross: f64 = leads.map(|l| l.max(0.0)).sum::<f64>() / games;
        let sample: Vec<f64> = fresh
            .iter()
            .filter(|&&(i, _)| switches(&differs[i], margin))
            .map(|&(_, lead)| lead)
            .collect();
        let per_game = switched.len() as f64 / games;
        let fresh_cost = if sample.is_empty() {
            "-".to_owned()
        } else {
            let (mean, error) = mean_and_error(&sample);
            format!(
                "{:+.4} ± {:.4} ({} rechecked)",
                per_game * mean,
                per_game * error,
                sample.len()
            )
        };
        let _ = writeln!(
            out,
            "{margin:<6}  {:>5} ({:>4.1}%)  {per_game:>6.1}  {net:+.4}, {gross:.4}  \
             {fresh_cost}",
            switched.len(),
            share(switched.len()),
        );
        if gross <= COST_MAX {
            recommended = Some(margin);
        }
    }
    let _ = writeln!(
        out,
        "\nrecommended margin by the logged cost (the largest costing at most {COST_MAX} a \
         game, gross): {}",
        recommended.map_or("none".to_owned(), |m| m.to_string())
    );
    let margin = 0.01;
    let _ = writeln!(out, "\nswitched at {margin}, by kind:");
    let mut kinds: Vec<(DecisionKind, usize, Vec<f64>)> = Vec::new();
    for (i, d) in differs.iter().enumerate() {
        if !switches(d, margin) {
            continue;
        }
        let k = d.decision.kind;
        let at = kinds.iter().position(|e| e.0 == k).unwrap_or_else(|| {
            kinds.push((k, 0, Vec::new()));
            kinds.len() - 1
        });
        kinds[at].1 += 1;
        kinds[at]
            .2
            .extend(fresh.iter().filter(|e| e.0 == i).map(|e| e.1));
    }
    kinds.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (kind, n, leads) in kinds {
        let fresh = if leads.is_empty() {
            String::new()
        } else {
            let (mean, error) = mean_and_error(&leads);
            format!("  fresh lead {mean:+.4} ± {error:.4} over {}", leads.len())
        };
        let _ = writeln!(
            out,
            "  {kind:?}: {n} ({:.1} a game){fresh}",
            n as f64 / games
        );
    }
    let _ = writeln!(out, "\nswitched at {margin}, the first {examples}:");
    for d in differs
        .iter()
        .filter(|d| switches(d, margin))
        .take(examples)
    {
        let _ = writeln!(out, "  {}  {}", d.game, d.line.trim());
    }
    out
}

/// The mean of `xs` and its standard error.
pub fn mean_and_error(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (mean, 0.0);
    }
    let variance = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, (variance / n).sqrt())
}
