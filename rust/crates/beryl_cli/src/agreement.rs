//! `beryl_cli agreement <run-dir> <entrant> [--positions N] [--budget B]`: how consistently a
//! long-think search chooses, as a measure of how randomly it plays. N of the entrant's
//! searched decisions in decided positions (its logged best mean above 0.95 or below 0.05) and
//! N in contested ones, spread evenly over the run's games, are each searched twice with the
//! entrant's family from two fixed seeds, at its budget or `B`. A search with a reason for its
//! move makes it from either seed; one choosing among moves it cannot tell apart, by noise,
//! agrees about as often as chance would. Also reported: the share of root visits the chosen
//! candidate took, and how often the logged move is repeated.

use std::fmt::Write as _;
use std::path::Path;

use beryl_engine::{Choice, Decision, Scratch, State, observe};
use beryl_search::{Agent, AgentDecision, Family, LongSearch, Settings};

use crate::anchor_replay::walk;
use crate::game::{mirror, parse_work};

/// The seeds the two searches of each position draw from.
const SEEDS: [u64; 2] = [0xA6EE_0001, 0xA6EE_0002];

/// A position with the move its log records and whether it was decided.
struct Position {
    state: State,
    decision: Decision,
    logged: u16,
    decided: bool,
}

pub fn main(args: &[String]) -> Result<(), String> {
    let usage =
        || "usage: beryl_cli agreement <run-dir> <entrant> [--positions N] [--budget B]".to_owned();
    let [dir, entrant, flags @ ..] = args else {
        return Err(usage());
    };
    let (mut n, mut budget) = (40usize, None);
    for pair in flags.chunks(2) {
        let [flag, value] = pair else {
            return Err(usage());
        };
        match flag.as_str() {
            "--positions" => n = value.parse().map_err(|_| usage())?,
            "--budget" => budget = Some(parse_work(value).ok_or_else(usage)?),
            _ => return Err(usage()),
        }
    }
    print!("{}", agreement(Path::new(dir), entrant, n, budget)?);
    Ok(())
}

fn agreement(dir: &Path, entrant: &str, n: usize, budget: Option<u64>) -> Result<String, String> {
    let bad = || format!("{entrant:?} is no long-think entrant (family@budget)");
    let (family, logged_budget) = entrant.split_once('@').ok_or_else(bad)?;
    let family: Family = family.parse()?;
    let work = budget.unwrap_or(parse_work(logged_budget).ok_or_else(bad)?);
    let mut positions = Vec::new();
    walk(dir, entrant, |_, s, d, choice, search, _| {
        let best = search
            .visits
            .iter()
            .zip(&search.rewards)
            .filter(|&(&v, _)| v > 0)
            .map(|(&v, &r)| r / f64::from(v))
            .fold(f64::NEG_INFINITY, f64::max);
        positions.push(Position {
            state: *s,
            decision: *d,
            logged: choice,
            decided: !(0.05..=0.95).contains(&best),
        });
    })?;
    let t = mirror();
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
    let settings = Settings {
        family,
        work,
        threads,
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} at {work} steps: {} searched decisions in the log, {} decided",
        family,
        positions.len(),
        positions.iter().filter(|p| p.decided).count()
    );
    for decided in [true, false] {
        let pool: Vec<&Position> = positions.iter().filter(|p| p.decided == decided).collect();
        let step = (pool.len() as f64 / n as f64).max(1.0);
        let chosen: Vec<&Position> = (0..n.min(pool.len()))
            .map(|k| pool[(k as f64 * step) as usize])
            .collect();
        let (mut agree, mut repeats, mut share, mut chance) = (0, 0, 0.0, 0.0);
        for p in &chosen {
            let mut x = Scratch::new();
            let picks = SEEDS.map(|seed| {
                let mut search = LongSearch::new(&t, settings, seed);
                let c = search.choose(
                    observe(&p.state, &t, &mut x, p.decision.player),
                    &p.decision,
                    AgentDecision(0),
                );
                let visits = search.last_search().map_or(0.0, |s| {
                    let total: u32 = s.visits.iter().sum();
                    f64::from(s.visits[usize::from(c.0)]) / f64::from(total.max(1))
                });
                (c, visits)
            });
            agree += usize::from(picks[0].0 == picks[1].0);
            chance += 1.0 / p.decision.len() as f64;
            repeats += picks.iter().filter(|(c, _)| *c == Choice(p.logged)).count();
            share += picks.iter().map(|(_, v)| v).sum::<f64>() / 2.0;
            eprintln!(
                "{} {}: {} {}",
                if decided { "decided" } else { "contested" },
                p.decision.len(),
                picks[0].0.0,
                picks[1].0.0
            );
        }
        let m = chosen.len().max(1) as f64;
        let _ = writeln!(
            out,
            "{:<9} {:>3} positions: the two seeds agree {:>5.1}% (by chance {:>4.1}%), repeat the \
             logged move {:>5.1}%, chosen candidate's share of root visits {:>5.1}%",
            if decided { "decided" } else { "contested" },
            chosen.len(),
            100.0 * agree as f64 / m,
            100.0 * chance / m,
            100.0 * repeats as f64 / (2.0 * m),
            100.0 * share / m
        );
    }
    Ok(out)
}
