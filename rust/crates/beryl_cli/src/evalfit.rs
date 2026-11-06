//! `beryl_cli eval-fit [--games N] [--seed N]`: fits the board evaluation's weights
//! (`beryl_search`'s [`WEIGHTS`]) by logistic regression on seeded mirror games, and prints
//! them with the fit's log loss and accuracy against a coin and against the weights in use.
//!
//! The games are the frozen expert against greedy, each in both seats, and greedy against
//! itself: two players of different strength, so a position's features carry contrast. Each
//! position is the state at the first decision of a turn, the features first seat less
//! second, labeled with whether the first seat won; drawn and stalled games are left out.

use std::thread;

use beryl_engine::{Outcome, Profile, Replay, Scratch, Seat, State, Step, apply, next, observe};
use beryl_search::{Budget, CardValues, FEATURES, NAMES, WEIGHTS, features, sigmoid};

use crate::game::{agents, mirror};

/// One position: the feature differences and whether the first seat won.
type Sample = ([f64; FEATURES], bool);

/// The positions of game `seed` between `names`.
fn positions(seed: u64, names: [&str; 2]) -> Result<Vec<Sample>, String> {
    let t = mirror();
    let values = CardValues::new(&t);
    let mut agents = agents(&t, names, seed, Budget::DEFAULT)?;
    let mut s = State::new(&t, seed, Profile::SMART);
    let mut x = Scratch::new();
    let mut turn = 0;
    let mut at_turns = Vec::new();
    let outcome = loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => {
                if s.turn().number != turn {
                    turn = s.turn().number;
                    let [a, b] = Seat::BOTH.map(|p| features(&s, &values, p).0);
                    let mut diff = [0.0; FEATURES];
                    for (i, d) in diff.iter_mut().enumerate() {
                        *d = a[i] - b[i];
                    }
                    at_turns.push(diff);
                }
                let c = agents[d.player.index()].choose(observe(&s, &t, &mut x, d.player), &d);
                apply(&mut s, &t, &mut x, &mut Replay, c);
            }
        }
    };
    let won = match outcome {
        Outcome::Win(w) => w == Seat::P0,
        Outcome::Draw | Outcome::Stalled(_) => return Ok(Vec::new()),
    };
    Ok(at_turns.into_iter().map(|f| (f, won)).collect())
}

/// Each game's positions, played in parallel.
fn sample(games: u32, seed: u64) -> Result<Vec<Vec<Sample>>, String> {
    let pairings: [[&str; 2]; 3] = [
        ["expert-frozen", "greedy"],
        ["greedy", "expert-frozen"],
        ["greedy", "greedy"],
    ];
    let specs: Vec<(u64, [&str; 2])> = (0..u64::from(games))
        .map(|g| (seed.wrapping_add(g), pairings[(g % 3) as usize]))
        .collect();
    let workers = thread::available_parallelism().map_or(1, usize::from);
    let chunks: Vec<&[(u64, [&str; 2])]> = specs.chunks(specs.len().div_ceil(workers)).collect();
    let results: Vec<Result<Vec<Vec<Sample>>, String>> = thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                scope.spawn(move || {
                    let mut out = Vec::new();
                    for &(seed, names) in *chunk {
                        out.push(positions(seed, names)?);
                    }
                    Ok(out)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a fitting thread"))
            .collect()
    });
    let mut all = Vec::new();
    for r in results {
        all.extend(r?);
    }
    Ok(all)
}

/// The weights of an L2-regularized logistic regression of `samples`, by Newton's method.
pub fn fit(samples: &[Sample], l2: f64) -> [f64; FEATURES] {
    let mut w = [0.0; FEATURES];
    for _ in 0..50 {
        let mut grad = [0.0; FEATURES];
        let mut hess = [[0.0; FEATURES]; FEATURES];
        for (x, y) in samples {
            let p = sigmoid(x.iter().zip(&w).map(|(x, w)| x * w).sum());
            let e = p - f64::from(u8::from(*y));
            for i in 0..FEATURES {
                grad[i] += e * x[i];
                for j in 0..FEATURES {
                    hess[i][j] += p * (1.0 - p) * x[i] * x[j];
                }
            }
        }
        for i in 0..FEATURES {
            grad[i] += l2 * w[i];
            hess[i][i] += l2;
        }
        let step = solve(hess, grad);
        let mut moved = 0.0f64;
        for i in 0..FEATURES {
            w[i] -= step[i];
            moved = moved.max(step[i].abs());
        }
        if moved < 1e-9 {
            break;
        }
    }
    w
}

/// `a`⁻¹ `b`, by Gaussian elimination with partial pivoting.
fn solve(mut a: [[f64; FEATURES]; FEATURES], mut b: [f64; FEATURES]) -> [f64; FEATURES] {
    for col in 0..FEATURES {
        let pivot = (col..FEATURES)
            .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
            .expect("a row");
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..FEATURES {
            let f = a[row][col] / a[col][col];
            let top = a[col];
            for (cell, above) in a[row].iter_mut().zip(top).skip(col) {
                *cell -= f * above;
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = [0.0; FEATURES];
    for row in (0..FEATURES).rev() {
        let rest: f64 = (row + 1..FEATURES).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - rest) / a[row][row];
    }
    x
}

/// Mean log loss and accuracy of weights `w` on `samples`.
fn score(samples: &[Sample], w: &[f64; FEATURES]) -> (f64, f64) {
    let mut loss = 0.0;
    let mut right = 0;
    for (x, y) in samples {
        let p = sigmoid(x.iter().zip(w).map(|(x, w)| x * w).sum()).clamp(1e-9, 1.0 - 1e-9);
        loss -= if *y { p.ln() } else { (1.0 - p).ln() };
        right += usize::from((p > 0.5) == *y);
    }
    let n = samples.len() as f64;
    (loss / n, right as f64 / n)
}

pub fn main(args: &[String]) -> Result<(), String> {
    let mut games = 6_000;
    let mut seed = 1;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = args.next().ok_or(format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--games" => games = crate::number(arg, value)?,
            "--seed" => seed = crate::number(arg, value)?,
            _ => return Err("usage: beryl_cli eval-fit [--games N] [--seed N]".to_owned()),
        }
    }
    let games_played = sample(games, seed)?;
    // Every fifth game is held out, its positions together.
    let (mut train, mut test) = (Vec::new(), Vec::new());
    for (g, positions) in games_played.iter().enumerate() {
        if g % 5 == 0 {
            test.extend_from_slice(positions);
        } else {
            train.extend_from_slice(positions);
        }
    }
    let w = fit(&train, 1.0);
    let (loss, accuracy) = score(&test, &w);
    let (now_loss, now_accuracy) = score(&test, &WEIGHTS);
    println!(
        "{} positions from {games} games ({} held out)",
        train.len() + test.len(),
        test.len()
    );
    for (name, w) in NAMES.iter().zip(&w) {
        println!("  {name:<16} {w:+.4}");
    }
    println!(
        "held out: log loss {loss:.4} (coin 0.6931, in use {now_loss:.4}), accuracy \
         {:.1}% (in use {:.1}%)",
        100.0 * accuracy,
        100.0 * now_accuracy
    );
    let rounded: Vec<String> = w.iter().map(|w| format!("{w:.4}")).collect();
    println!(
        "pub const WEIGHTS: [f64; FEATURES] = [{}];",
        rounded.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples drawn from known weights fit back to about those weights.
    #[test]
    fn a_fit_recovers_known_weights() {
        let truth = [0.5, -1.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0];
        let mut rng = beryl_engine::rng::SplitMix64::new(3);
        let mut uniform = || (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        let samples: Vec<Sample> = (0..20_000)
            .map(|_| {
                let mut x = [0.0; FEATURES];
                for v in &mut x {
                    *v = 4.0 * uniform() - 2.0;
                }
                let p = sigmoid(x.iter().zip(&truth).map(|(x, w)| x * w).sum());
                (x, uniform() < p)
            })
            .collect();
        let w = fit(&samples, 1e-3);
        for (got, want) in w.iter().zip(&truth) {
            assert!((got - want).abs() < 0.06, "{w:?}");
        }
    }
}
