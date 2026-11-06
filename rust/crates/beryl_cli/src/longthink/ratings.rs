//! Where every entrant stands: one Bradley-Terry rating pool over all the run's games, in
//! Elo, with the anchor fixed at 0, read by the schedule, the summary and the live panel
//! alike.
//!
//! - The ratings are the maximum a posteriori fit of P(a beats b) = 1 / (1 + 10^((rb −
//!   ra) / 400)) to every scored game, a draw or a stall half a win, under a normal prior
//!   of 400 Elo on each entrant but the anchor, so that a sweep stays finite. Newton's method
//!   finds it; the inverse of the negated Hessian there is the ratings' covariance (the
//!   Laplace approximation), so a difference between any two entrants has a margin.
//! - An entrant's tally keeps what the views print besides: games, score, the work its
//!   searches spent and the wall time.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use beryl_engine::rng::SplitMix64;

use super::schedule::{Config, family_of};
use super::{GameRecord, Verdict};

/// The prior's standard deviation on a rating, in Elo.
pub const PRIOR: f64 = 400.0;

/// Elo to natural log-odds.
pub const K: f64 = std::f64::consts::LN_10 / 400.0;

/// Draws of the ratings behind each chance of being best.
const DRAWS: u32 = 2_000;

/// One entrant's games, from its side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    /// Games played, crashed ones included.
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    /// Its searched decisions, their iterations and the engine steps they spent.
    pub searched: u64,
    pub iterations: u64,
    pub work: u64,
    /// The games with a wall time, and their seconds.
    pub clocked: u32,
    pub seconds: f64,
}

impl Tally {
    pub fn scored(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    /// Mean engine steps its games cost it; none before a game.
    pub fn work_per_game(&self) -> Option<f64> {
        (self.games > 0).then(|| self.work as f64 / f64::from(self.games))
    }

    pub fn mean_seconds(&self) -> Option<f64> {
        (self.clocked > 0).then(|| self.seconds / f64::from(self.clocked))
    }
}

/// The rating pool.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ratings {
    /// Every entrant rated, in order: the anchor, then the rest by name.
    names: Vec<String>,
    index: HashMap<String, usize>,
    mean: Vec<f64>,
    /// The ratings' covariance, the anchor's row and column zero.
    cov: Vec<Vec<f64>>,
    tallies: Vec<Tally>,
    /// Each matchup's games so far, by its sides.
    played: BTreeMap<(String, String), u32>,
}

impl Ratings {
    /// The pool after `games`, with `also` rated at the prior if they have not played, and
    /// each game's wall seconds if `clock` has them.
    pub fn new(
        c: &Config,
        games: &[GameRecord],
        clock: &HashMap<String, f64>,
        also: &[String],
    ) -> Ratings {
        let mut rest: BTreeSet<String> = also.iter().cloned().collect();
        for g in games {
            rest.insert(g.a.clone());
            rest.insert(g.b.clone());
        }
        rest.remove(&c.anchor);
        let names: Vec<String> = std::iter::once(c.anchor.clone()).chain(rest).collect();
        let index: HashMap<String, usize> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        let n = names.len();
        let mut tallies = vec![Tally::default(); n];
        let mut played = BTreeMap::new();
        // Each pair of entrants' points and scored games, a first.
        let mut results: BTreeMap<(usize, usize), (f64, f64)> = BTreeMap::new();
        for g in games {
            *played.entry((g.a.clone(), g.b.clone())).or_insert(0) += 1;
            let (a, b) = (index[&g.a], index[&g.b]);
            let seconds = clock.get(&g.game).copied();
            for (side, i) in [(0, a), (1, b)] {
                let t = &mut tallies[i];
                t.games += 1;
                let mine = if side == 0 {
                    g.outcome
                } else {
                    g.outcome.flipped()
                };
                match mine {
                    Verdict::Win => t.wins += 1,
                    Verdict::Loss => t.losses += 1,
                    Verdict::Draw | Verdict::Stalled => t.draws += 1,
                    Verdict::Crashed => {}
                }
                t.searched += u64::from(g.searched[side]);
                t.iterations += g.iterations[side];
                t.work += g.work[side];
                if let Some(s) = seconds {
                    t.clocked += 1;
                    t.seconds += s;
                }
            }
            let y = match g.outcome {
                Verdict::Win => 1.0,
                Verdict::Loss => 0.0,
                Verdict::Draw | Verdict::Stalled => 0.5,
                Verdict::Crashed => continue,
            };
            let r = results.entry((a, b)).or_insert((0.0, 0.0));
            r.0 += y;
            r.1 += 1.0;
        }
        let (mean, cov) = fit(n, &results);
        Ratings {
            names,
            index,
            mean,
            cov,
            tallies,
            played,
        }
    }

    /// Every entrant rated, the anchor first.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Entrant `e`'s rating and its standard deviation; the prior's for one not rated.
    pub fn get(&self, e: &str) -> (f64, f64) {
        self.index
            .get(e)
            .map_or((0.0, PRIOR), |&i| (self.mean[i], self.cov[i][i].sqrt()))
    }

    /// The variance of `e`'s rating and its covariance with `f`'s.
    pub fn var_cov(&self, e: &str, f: &str) -> (f64, f64) {
        match (self.index.get(e), self.index.get(f)) {
            (Some(&i), Some(&j)) => (self.cov[i][i], self.cov[i][j]),
            (Some(&i), None) => (self.cov[i][i], 0.0),
            (None, _) => (PRIOR * PRIOR, 0.0),
        }
    }

    /// `e`'s rating less `f`'s, and the difference's standard deviation.
    pub fn diff(&self, e: &str, f: &str) -> (f64, f64) {
        let (ve, cef) = self.var_cov(e, f);
        let (vf, _) = self.var_cov(f, e);
        let (me, mf) = (self.get(e).0, self.get(f).0);
        (me - mf, (ve + vf - 2.0 * cef).max(0.0).sqrt())
    }

    /// Entrant `e`'s games.
    pub fn tally(&self, e: &str) -> Tally {
        self.index
            .get(e)
            .map_or_else(Tally::default, |&i| self.tallies[i])
    }

    /// Games of the matchup with sides `a` and `b`, in that order.
    pub fn played(&self, a: &str, b: &str) -> u32 {
        self.played
            .get(&(a.to_owned(), b.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    /// Each of `among`'s chance of the best rating among them, and of the best in its
    /// family, from draws of the ratings seeded by `seed`, correlated as the pool's
    /// covariance says: two entrants that have played each other often differ by a known
    /// margin, however uncertain each is against the anchor.
    pub fn best(&self, among: &[String], seed: u64) -> Vec<(f64, f64)> {
        if among.is_empty() {
            return Vec::new();
        }
        let n = among.len();
        let cov: Vec<Vec<f64>> = among
            .iter()
            .map(|e| among.iter().map(|f| self.covariance(e, f)).collect())
            .collect();
        let factor = cholesky(&cov);
        let means: Vec<f64> = among.iter().map(|e| self.get(e).0).collect();
        let mut rng = SplitMix64::new(seed);
        let families: Vec<&str> = among.iter().map(|e| family_of(e)).collect();
        let mut overall = vec![0u32; n];
        let mut in_family = vec![0u32; n];
        let mut z = vec![0.0; n];
        let mut draw = vec![0.0; n];
        for _ in 0..DRAWS {
            for v in &mut z {
                *v = normal(&mut rng);
            }
            for (i, d) in draw.iter_mut().enumerate() {
                *d = means[i] + (0..=i).map(|j| factor[i][j] * z[j]).sum::<f64>();
            }
            let top = argmax(&draw, |_| true);
            overall[top] += 1;
            let mut counted = BTreeSet::new();
            for &f in &families {
                if counted.insert(f) {
                    let top = argmax(&draw, |i| families[i] == f);
                    in_family[top] += 1;
                }
            }
        }
        overall
            .iter()
            .zip(&in_family)
            .map(|(&o, &f)| {
                (
                    f64::from(o) / f64::from(DRAWS),
                    f64::from(f) / f64::from(DRAWS),
                )
            })
            .collect()
    }

    /// The covariance of `e`'s and `f`'s ratings; an entrant not rated has the prior's
    /// variance and no covariance.
    fn covariance(&self, e: &str, f: &str) -> f64 {
        match (self.index.get(e), self.index.get(f)) {
            (Some(&i), Some(&j)) => self.cov[i][j],
            _ if e == f => PRIOR * PRIOR,
            _ => 0.0,
        }
    }
}

/// The lower-triangular `l` with `l lᵀ = a`, for a covariance `a`; a pivot that rounding
/// leaves at or below zero (the anchor's, which is fixed) is taken as zero.
fn cholesky(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let dot: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                l[i][i] = (a[i][i] - dot).max(0.0).sqrt();
            } else if l[j][j] > 1e-9 {
                l[i][j] = (a[i][j] - dot) / l[j][j];
            }
        }
    }
    l
}

/// The index of the largest of `xs` that `keep` keeps, the first of equals.
fn argmax(xs: &[f64], keep: impl Fn(usize) -> bool) -> usize {
    let mut best: Option<usize> = None;
    for (i, &x) in xs.iter().enumerate() {
        if keep(i) && best.is_none_or(|b| x > xs[b]) {
            best = Some(i);
        }
    }
    best.expect("one kept")
}

/// A standard normal draw, by Box-Muller.
fn normal(rng: &mut SplitMix64) -> f64 {
    let u = |rng: &mut SplitMix64| ((rng.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    let (a, b) = (u(rng), u(rng));
    (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
}

/// The ratings of `n` entrants, the first the anchor at 0, from each pair's points and games
/// (the first's), and their covariance.
fn fit(n: usize, results: &BTreeMap<(usize, usize), (f64, f64)>) -> (Vec<f64>, Vec<Vec<f64>>) {
    let mut r = vec![0.0; n];
    let free = n - 1;
    let mut cov = vec![vec![0.0; n]; n];
    if free == 0 {
        return (r, cov);
    }
    let prior = 1.0 / (PRIOR * PRIOR);
    let mut info = vec![vec![0.0; free]; free];
    for _ in 0..100 {
        let mut grad = vec![0.0; free];
        info = vec![vec![0.0; free]; free];
        for (i, row) in info.iter_mut().enumerate() {
            grad[i] -= prior * r[i + 1];
            row[i] += prior;
        }
        for (&(a, b), &(points, games)) in results {
            let p = 1.0 / (1.0 + (-K * (r[a] - r[b])).exp());
            let g = K * (points - games * p);
            let h = K * K * games * p * (1.0 - p);
            for (x, sign) in [(a, 1.0), (b, -1.0)] {
                if x > 0 {
                    grad[x - 1] += sign * g;
                }
            }
            for (x, y, sign) in [(a, a, 1.0), (b, b, 1.0), (a, b, -1.0), (b, a, -1.0)] {
                if x > 0 && y > 0 {
                    info[x - 1][y - 1] += sign * h;
                }
            }
        }
        let step = solve(&info, &grad);
        let mut moved = 0.0f64;
        for (i, s) in step.iter().enumerate() {
            r[i + 1] += s;
            moved = moved.max(s.abs());
        }
        if moved < 1e-6 {
            break;
        }
    }
    let inverse = invert(&info);
    for i in 0..free {
        for j in 0..free {
            cov[i + 1][j + 1] = inverse[i][j];
        }
    }
    (r, cov)
}

/// `a`⁻¹ `b` for a symmetric positive definite `a`, by Gaussian elimination.
fn solve(a: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a
        .iter()
        .zip(b)
        .map(|(row, &v)| {
            let mut r = row.clone();
            r.push(v);
            r
        })
        .collect();
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))
            .expect("a row");
        m.swap(col, pivot);
        for row in 0..n {
            if row != col {
                let f = m[row][col] / m[col][col];
                let top = m[col].clone();
                for (cell, above) in m[row].iter_mut().zip(top).skip(col) {
                    *cell -= f * above;
                }
            }
        }
    }
    (0..n).map(|i| m[i][n] / m[i][i]).collect()
}

/// `a`⁻¹, a column at a time.
fn invert(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let columns: Vec<Vec<f64>> = (0..n)
        .map(|j| {
            let e: Vec<f64> = (0..n).map(|i| f64::from(u8::from(i == j))).collect();
            solve(a, &e)
        })
        .collect();
    (0..n)
        .map(|i| (0..n).map(|j| columns[j][i]).collect())
        .collect()
}

/// Elo per tripling of the budget, and its 95% margin: the weighted least-squares slope of
/// ratings against log3 of their budgets, from (budget, rating, standard deviation); none
/// before two budgets.
pub fn climb(points: &[(u64, f64, f64)]) -> Option<(f64, f64)> {
    let pts: Vec<(f64, f64, f64)> = points
        .iter()
        .map(|&(b, r, s)| ((b as f64).log(3.0), r, s.max(1.0)))
        .collect();
    let w: Vec<f64> = pts.iter().map(|&(_, _, s)| 1.0 / (s * s)).collect();
    let total: f64 = w.iter().sum();
    let mean = |f: fn(&(f64, f64, f64)) -> f64| {
        pts.iter().zip(&w).map(|(p, w)| w * f(p)).sum::<f64>() / total
    };
    let (x, y) = (mean(|p| p.0), mean(|p| p.1));
    let sxx: f64 = pts.iter().zip(&w).map(|(p, w)| w * (p.0 - x).powi(2)).sum();
    if pts.len() < 2 || sxx <= 1e-12 {
        return None;
    }
    let sxy: f64 = pts
        .iter()
        .zip(&w)
        .map(|(p, w)| w * (p.0 - x) * (p.1 - y))
        .sum();
    Some((sxy / sxx, 1.96 / sxx.sqrt()))
}

/// Elo per tripling as the summary and the panel print it: `+103 ±47`.
pub fn climb_text((slope, margin): (f64, f64)) -> String {
    // Rounded first, so that a slope a hair below zero reads +0, not -0.
    let slope = slope.round() + 0.0;
    format!("{slope:+.0} ±{margin:.0}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    /// One entrant against the anchor: a 75% score is 400 log10 3 = 191 Elo, shrunk a
    /// little toward the prior, its margin about the delta method's.
    #[test]
    fn a_score_against_the_anchor_is_its_elo() {
        let mut results = BTreeMap::new();
        results.insert((1, 0), (300.0, 400.0));
        let (r, cov) = fit(2, &results);
        assert!(r[0].abs() < f64::EPSILON);
        assert!(close(r[1], 190.8, 2.0), "{r:?}");
        // The delta method: 400 / (ln 10 √(n p (1 − p))) = 20.05.
        assert!(close(cov[1][1].sqrt(), 20.05, 0.5), "{cov:?}");
    }

    /// Ratings chain: a beats b as b beats the anchor, so a sits about twice as high, and
    /// its difference from b is known better than either rating.
    #[test]
    fn ratings_chain_through_common_opponents() {
        let mut results = BTreeMap::new();
        results.insert((2, 0), (300.0, 400.0));
        results.insert((1, 2), (300.0, 400.0));
        let (r, cov) = fit(3, &results);
        assert!(close(r[2], 190.8, 3.0) && close(r[1], 381.6, 6.0), "{r:?}");
        let diff = (cov[1][1] + cov[2][2] - 2.0 * cov[1][2]).sqrt();
        assert!(diff < cov[1][1].sqrt(), "{cov:?}");
    }

    /// A sweep stays finite under the prior.
    #[test]
    fn a_sweep_stays_finite() {
        let mut results = BTreeMap::new();
        results.insert((1, 0), (10.0, 10.0));
        let (r, _) = fit(2, &results);
        assert!(r[1].is_finite() && r[1] > 200.0 && r[1] < 2_000.0, "{r:?}");
    }

    #[test]
    fn elo_per_tripling_is_a_weighted_slope() {
        let (s, m) = climb(&[(1_000, 0.0, 10.0), (3_000, 100.0, 10.0)]).unwrap();
        assert!(close(s, 100.0, 1e-9) && close(m, 27.718, 1e-3), "{s} {m}");
        assert_eq!(climb(&[(1_000, 0.0, 10.0)]), None);
        assert_eq!(climb_text((-1e-14, 47.2)), "+0 ±47");
    }

    /// Two entrants each known poorly against the anchor, but one known to beat the other:
    /// the correlated draws make the better one best nearly always.
    #[test]
    fn the_chance_of_being_best_reads_the_covariance() {
        use crate::longthink::schedule::Matchup;
        let c = Config {
            families: Vec::new(),
            fixed: vec!["expert".to_owned()],
            anchor: "expert".to_owned(),
            budgets: vec![1_000],
            threads: 1,
            seed: 0,
        };
        let (a, b) = ("f/x@1k", "f/y@1k");
        let mut games = Vec::new();
        for (m, n, wins) in [
            (Matchup::new(a, b), 200, 150),
            (Matchup::new("expert", a), 2, 1),
            (Matchup::new("expert", b), 2, 1),
        ] {
            for g in 0..n {
                let v = if g < wins {
                    Verdict::Win
                } else {
                    Verdict::Loss
                };
                games.push(GameRecord::example(&m, g, v));
            }
        }
        let r = Ratings::new(&c, &games, &HashMap::new(), &[]);
        let (_, sa) = r.get(a);
        let (gap, sd) = r.diff(a, b);
        assert!(sa > 3.0 * sd && gap > 2.0 * sd, "{sa} {gap} {sd}");
        let best = r.best(&[a.to_owned(), b.to_owned()], 1);
        assert!(best[0].0 > 0.97, "{best:?}");
    }

    #[test]
    fn normal_draws_have_unit_variance() {
        let mut rng = SplitMix64::new(9);
        let xs: Vec<f64> = (0..20_000).map(|_| normal(&mut rng)).collect();
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
        assert!(mean.abs() < 0.03 && close(var, 1.0, 0.05), "{mean} {var}");
    }
}
