//! `beryl_cli value-test <match-dir>... [--seed N] [--out DIR]`: whether a small model
//! trained on who won calls the winner of a searched position better than the search's own
//! root estimate at the same decision. A learned value model could only improve on rollouts
//! if it did.
//!
//! The samples are each searching entrant's first searched decision of each turn in the
//! matches' logs, labelled with whether the deciding player won. Drawn, stalled and crashed
//! games are left out. Four predictors are scored, each on five folds of the games' groups
//! (a start position, or a game seed without one) by a model trained on the other four:
//!
//! - `search`: the search's mean reward for the move it played, through a fitted
//!   `sigmoid(a * logit(x) + b)`, so that the turn discount's squeeze toward 0.5 does not
//!   count against it;
//! - `linear`: a logistic regression on the features of [`features::Columns`];
//! - `mlp`: one hidden layer of 32 `tanh` units on the same features;
//! - `mlp+search`: `mlp` with the logit of the search's estimate as one more input.
//!
//! The report ([`report`]) goes to standard output and to `value.json` in `--out`.

pub mod features;
pub mod model;
pub mod report;

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::thread;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Scratch, observe};
use beryl_search::Family;

use crate::Catalog;
use crate::anchor_replay::{games, walk_games};
use crate::game::{mirror, parse_work};
use crate::match_log::files;
use model::{LINEAR, MLP, Net, Shape, logit};

const USAGE: &str = "usage: beryl_cli value-test <match-dir>... [--seed N] [--out DIR]";

/// How many folds the groups are dealt into.
pub const FOLDS: usize = 5;

/// The predictors, in the report's order.
pub const PREDICTORS: [&str; 4] = ["search", "linear", "mlp", "mlp+search"];

/// One searched decision: what the decider saw, what the search estimated and who won.
#[derive(Clone, Debug)]
pub struct Sample {
    /// The game's start position name, or its seed: games that share one share a fold.
    pub group: String,
    /// The game, as `<match-dir index>/<game>`.
    pub game: String,
    /// The deciding entrant's budget, in engine steps.
    pub budget: u64,
    pub features: Vec<f64>,
    /// The search's mean reward for the move it played, for the decider.
    pub search: f64,
    pub won: bool,
}

/// The samples of the match in `dir`, its games named under `index`, and how many searched
/// decisions had no estimate for the move played.
pub fn collect(
    catalog: &'static Catalog,
    dir: &Path,
    index: usize,
) -> Result<(Vec<Sample>, usize), String> {
    let t = mirror(catalog);
    let columns = features::Columns::new(&t);
    let mut entrants = BTreeSet::new();
    for g in games(dir)? {
        entrants.insert(g.a);
        entrants.insert(g.b);
    }
    let mut samples = Vec::new();
    let mut unestimated = 0;
    for entrant in entrants {
        let Some(budget) = entrant
            .rsplit_once('@')
            .filter(|(family, _)| family.parse::<Family>().is_ok())
            .and_then(|(_, work)| parse_work(work))
        else {
            continue;
        };
        let mut x = Scratch::new();
        let mut last = None;
        walk_games(catalog, dir, &entrant, |g, s, d, choice, search| {
            let won = match g.outcome.as_str() {
                "win" => d.player.index() == usize::from(g.seat),
                "loss" => d.player.index() != usize::from(g.seat),
                _ => return,
            };
            let turn = (g.game.clone(), d.player, s.turn().number());
            if last.as_ref() == Some(&turn) {
                return;
            }
            last = Some(turn);
            // For a decision the whole-turn search answered (`beryl_search`'s
            // `long/turn.rs`), each candidate is a root line and its mean is that line's mean
            // reward over the scoring worlds, so this reads the played line's mean too.
            let c = usize::from(choice);
            let mean = search.means.get(c).copied().flatten().or_else(|| {
                let visits = f64::from(search.visits.get(c).copied().unwrap_or(0));
                (visits > 0.0).then(|| search.rewards[c] / visits)
            });
            let Some(mean) = mean else {
                unestimated += 1;
                return;
            };
            samples.push(Sample {
                group: g
                    .position
                    .clone()
                    .unwrap_or_else(|| format!("seed {}", g.seed)),
                game: format!("{index}/{}", g.game),
                budget,
                features: columns.read(observe(s, &t, &mut x, d.decider)),
                search: mean,
                won,
            });
        })?;
    }
    Ok((samples, unestimated))
}

/// The fold of each sample: the groups in an order shuffled by `seed`, dealt round the folds
/// so each fold holds a fifth of them.
pub fn folds(samples: &[Sample], seed: u64) -> Vec<usize> {
    let hash = |group: &str| {
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        group.hash(&mut h);
        h.finish()
    };
    let groups: BTreeSet<(u64, &str)> = samples
        .iter()
        .map(|s| (hash(&s.group), s.group.as_str()))
        .collect();
    let fold: BTreeMap<&str, usize> = groups
        .into_iter()
        .enumerate()
        .map(|(i, (_, g))| (g, i % FOLDS))
        .collect();
    samples.iter().map(|s| fold[s.group.as_str()]).collect()
}

/// Each predictor's prediction for each sample, made by a model trained on the other folds.
pub fn predict(samples: &[Sample], seed: u64) -> Result<[Vec<f64>; 4], String> {
    let fold = folds(samples, seed);
    let used: BTreeSet<usize> = fold.iter().copied().collect();
    if used.len() < 2 {
        return Err(format!(
            "{} groups: at least two are needed, one to train on while the other is scored",
            used.len()
        ));
    }
    let searched: Vec<Vec<f64>> = samples.iter().map(|s| vec![logit(s.search)]).collect();
    let both: Vec<Vec<f64>> = samples
        .iter()
        .map(|s| {
            let mut f = s.features.clone();
            f.push(logit(s.search));
            f
        })
        .collect();
    let features: Vec<&[f64]> = samples.iter().map(|s| s.features.as_slice()).collect();
    let searched: Vec<&[f64]> = searched.iter().map(Vec::as_slice).collect();
    let both: Vec<&[f64]> = both.iter().map(Vec::as_slice).collect();
    let calibration = Shape {
        hidden: 0,
        l2: 1e-6,
    };
    let jobs: [(Shape, &[&[f64]]); 4] = [
        (calibration, &searched),
        (LINEAR, &features),
        (MLP, &features),
        (MLP, &both),
    ];
    let mut out: [Vec<f64>; 4] = std::array::from_fn(|_| vec![0.5; samples.len()]);
    let results: Vec<Vec<(usize, [f64; 4])>> = thread::scope(|scope| {
        let handles: Vec<_> = used
            .iter()
            .map(|&k| {
                let (fold, jobs) = (&fold, &jobs);
                scope.spawn(move || {
                    let (train, valid) = split(samples, fold, k, seed);
                    let held: Vec<usize> = (0..samples.len()).filter(|&i| fold[i] == k).collect();
                    let nets: Vec<Net> = jobs
                        .iter()
                        .enumerate()
                        .map(|(j, (shape, xs))| {
                            let rows = |is: &[usize]| -> Vec<(&[f64], bool)> {
                                is.iter().map(|&i| (xs[i], samples[i].won)).collect()
                            };
                            let seed = seed ^ ((k as u64) << 8) ^ j as u64;
                            Net::train(*shape, &rows(&train), &rows(&valid), seed)
                        })
                        .collect();
                    held.into_iter()
                        .map(|i| {
                            let p: [f64; 4] =
                                std::array::from_fn(|j| nets[j].predict(jobs[j].1[i]));
                            (i, p)
                        })
                        .collect()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a fold's thread"))
            .collect()
    });
    for (i, p) in results.into_iter().flatten() {
        for j in 0..4 {
            out[j][i] = p[j];
        }
    }
    Ok(out)
}

/// The training and early-stopping samples for fold `k`: every other fold's, with a tenth
/// of their groups (at least one, when there are two or more) held back to stop on.
fn split(samples: &[Sample], fold: &[usize], k: usize, seed: u64) -> (Vec<usize>, Vec<usize>) {
    let mut groups: Vec<&str> = samples
        .iter()
        .zip(fold)
        .filter(|(_, f)| **f != k)
        .map(|(s, _)| s.group.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut rng = SplitMix64::new(seed ^ 0x0057_A11D ^ k as u64);
    for i in (1..groups.len()).rev() {
        groups.swap(i, rng.below(i as u64 + 1) as usize);
    }
    let n = match groups.len() {
        0 | 1 => 0,
        n => (n / 10).max(1),
    };
    let stop: BTreeSet<&str> = groups[..n].iter().copied().collect();
    let (mut train, mut valid) = (Vec::new(), Vec::new());
    for (i, s) in samples.iter().enumerate() {
        if fold[i] == k {
            continue;
        }
        if stop.contains(s.group.as_str()) {
            valid.push(i);
        } else {
            train.push(i);
        }
    }
    (train, valid)
}

/// The whole test over `dirs` from `seed`.
pub fn run(
    catalog: &'static Catalog,
    dirs: &[PathBuf],
    seed: u64,
) -> Result<report::Report, String> {
    let mut samples = Vec::new();
    let mut unestimated = 0;
    for (i, dir) in dirs.iter().enumerate() {
        let (s, u) = collect(catalog, dir, i)?;
        samples.extend(s);
        unestimated += u;
    }
    if samples.is_empty() {
        return Err("no searched decisions in decided games".to_owned());
    }
    let predictions = predict(&samples, seed)?;
    Ok(report::Report::new(
        &samples,
        &predictions,
        unestimated,
        seed,
    ))
}

pub fn main(catalog: &'static Catalog, args: &[String]) -> Result<(), String> {
    let mut dirs = Vec::new();
    let mut seed = 1;
    let mut out = PathBuf::from(".");
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => seed = crate::number(arg, args.next().ok_or(USAGE)?)?,
            "--out" => out = PathBuf::from(args.next().ok_or(USAGE)?),
            s if s.starts_with('-') => return Err(USAGE.to_owned()),
            s => dirs.push(PathBuf::from(s)),
        }
    }
    if dirs.is_empty() {
        return Err(USAGE.to_owned());
    }
    let report = run(catalog, &dirs, seed)?;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    files::write_whole(&out.join("value.json"), report.json().as_bytes())?;
    print!("{}", report.text());
    Ok(())
}
