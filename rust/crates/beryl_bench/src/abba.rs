//! Interleaved timing of two versions in one executable: is B faster than A, and by how much?
//!
//! Timing A for ten seconds and then B for ten seconds compares A's ten seconds with B's:
//! anything that drifts in between (heat, clock speed, another process waking up) lands on
//! one side. Here each round times short blocks in the order A B B A, or B A A B on odd
//! rounds. Both versions' blocks then sit at the same average time in every round, so drift
//! that is roughly linear over a round cancels, and so does any advantage of going first.
//!
//! Each round gives one ratio, B's time over A's. The report is the median ratio with a
//! distribution-free confidence interval, so a round hit by an interruption, or by the
//! thread landing on a slower core, moves the answer very little; such rounds are counted
//! as disturbed.

use std::hint::black_box;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Config {
    /// Rounds of four blocks each.
    pub rounds: usize,
    /// Roughly how long one block takes: long against the clock's resolution (42 ns on
    /// Apple silicon), short against drift.
    pub block: Duration,
    pub warm_up: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rounds: 300,
            block: Duration::from_millis(5),
            warm_up: Duration::from_secs(2),
        }
    }
}

impl Config {
    /// Reads `--rounds N`, `--block-ms N`, `--warm-up-ms N` and `--quick` (30 rounds), and
    /// ignores the rest, such as the `--bench` that `cargo bench` passes.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut config = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut number = || args.next().and_then(|v| v.parse::<u64>().ok());
            match arg.as_str() {
                "--quick" => config.rounds = 30,
                "--rounds" => config.rounds = number().map_or(config.rounds, |n| n as usize),
                "--block-ms" => {
                    config.block = number().map_or(config.block, Duration::from_millis);
                }
                "--warm-up-ms" => {
                    config.warm_up = number().map_or(config.warm_up, Duration::from_millis);
                }
                _ => {}
            }
        }
        config.rounds = config.rounds.max(10);
        config
    }
}

#[derive(Clone, Debug)]
pub struct Report {
    pub name: String,
    /// Calls per block.
    pub iterations: u64,
    /// Median time of one call.
    pub a: Duration,
    pub b: Duration,
    /// Median of the per-round ratios B / A: below 1 means B is faster.
    pub ratio: f64,
    /// 95% confidence interval for that median.
    pub low: f64,
    pub high: f64,
    pub rounds: usize,
    /// Rounds with a block more than 25% off its version's median block.
    pub disturbed: usize,
}

impl Report {
    pub fn verdict(&self) -> String {
        let percent = |r: f64| (1.0 - r).abs() * 100.0;
        if self.high < 1.0 {
            format!("B is {:.1}% faster", percent(self.ratio))
        } else if self.low > 1.0 {
            format!("B is {:.1}% slower", percent(self.ratio))
        } else {
            "no difference detected (the interval includes 1)".to_owned()
        }
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{}: A {:.1?}, B {:.1?} per call",
            self.name, self.a, self.b
        )?;
        writeln!(
            f,
            "  B/A = {:.4} (95% CI {:.4} to {:.4}; {} ABBA rounds of {} calls per block)",
            self.ratio, self.low, self.high, self.rounds, self.iterations
        )?;
        writeln!(f, "  {}", self.verdict())?;
        if self.disturbed * 10 > self.rounds {
            writeln!(
                f,
                "  note: {} of {} rounds disturbed by other load or a slower core; the interval \
                 allows for them, and more --rounds narrow it",
                self.disturbed, self.rounds
            )?;
        }
        Ok(())
    }
}

/// Times `a` against `b` under `config`. Both should do the same work on the same inputs;
/// their results go through [`black_box`] so the compiler can't drop the work.
pub fn compare<T, U>(
    name: &str,
    config: &Config,
    mut a: impl FnMut() -> T,
    mut b: impl FnMut() -> U,
) -> Report {
    let mut block_a = |n: u64| time(n, &mut a);
    let mut block_b = |n: u64| time(n, &mut b);

    // Calibrate: double the calls per block until the slower version's block is long enough.
    let mut iterations = 1;
    while block_a(iterations).max(block_b(iterations)) < config.block.as_secs_f64() {
        iterations *= 2;
    }
    // Warm up caches, branch predictors and clock speed on both before measuring.
    let started = Instant::now();
    while started.elapsed() < config.warm_up {
        block_a(iterations);
        block_b(iterations);
    }

    let mut a_blocks = Vec::with_capacity(2 * config.rounds);
    let mut b_blocks = Vec::with_capacity(2 * config.rounds);
    for round in 0..config.rounds {
        let (a1, b1, b2, a2) = if round % 2 == 0 {
            let a1 = block_a(iterations);
            let b1 = block_b(iterations);
            let b2 = block_b(iterations);
            (a1, b1, b2, block_a(iterations))
        } else {
            let b1 = block_b(iterations);
            let a1 = block_a(iterations);
            let a2 = block_a(iterations);
            (a1, b1, block_b(iterations), a2)
        };
        a_blocks.push([a1, a2]);
        b_blocks.push([b1, b2]);
    }
    summarize(name, iterations, &a_blocks, &b_blocks)
}

/// Seconds for `n` calls.
fn time<T>(n: u64, f: &mut impl FnMut() -> T) -> f64 {
    let start = Instant::now();
    for _ in 0..n {
        black_box(f());
    }
    start.elapsed().as_secs_f64()
}

fn summarize(name: &str, iterations: u64, a: &[[f64; 2]], b: &[[f64; 2]]) -> Report {
    let mut ratios: Vec<f64> = a
        .iter()
        .zip(b)
        .map(|(a, b)| (b[0] + b[1]) / (a[0] + a[1]))
        .collect();
    ratios.sort_by(f64::total_cmp);
    let n = ratios.len();
    // Order statistics around the median: the binomial (normal-approximated) 95% interval.
    let k = ((n as f64 - 1.96 * (n as f64).sqrt()) / 2.0)
        .floor()
        .max(0.0) as usize;
    let a_median = median(a.iter().flatten().copied().collect());
    let b_median = median(b.iter().flatten().copied().collect());
    let off = |t: f64, m: f64| (t / m - 1.0).abs() > 0.25;
    let disturbed = a
        .iter()
        .zip(b)
        .filter(|(a, b)| a.iter().any(|&t| off(t, a_median)) || b.iter().any(|&t| off(t, b_median)))
        .count();
    let per_call = |t: f64| Duration::from_secs_f64(t / iterations as f64);
    Report {
        name: name.to_owned(),
        iterations,
        a: per_call(a_median),
        b: per_call(b_median),
        ratio: median(ratios.clone()),
        low: ratios[k],
        high: ratios[n - 1 - k],
        rounds: n,
        disturbed,
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, summarize};
    use std::time::Duration;

    #[test]
    fn a_steady_difference_is_detected_despite_one_bad_round() {
        let mut a = vec![[1.0, 1.0]; 50];
        let mut b = vec![[0.9, 0.9]; 50];
        a[3] = [5.0, 1.0]; // an interruption
        b[7] = [3.0, 0.9];
        let report = summarize("t", 1, &a, &b);
        assert!((report.ratio - 0.9).abs() < 1e-9);
        assert!(report.high < 1.0);
        assert_eq!(report.disturbed, 2);
        assert!(report.verdict().starts_with("B is 10.0% faster"));
    }

    #[test]
    fn noise_alone_is_no_difference() {
        let a: Vec<[f64; 2]> = (0..40)
            .map(|i| [1.0 + f64::from(i % 5) * 0.01; 2])
            .collect();
        let b: Vec<[f64; 2]> = (0..40)
            .map(|i| [1.0 + f64::from(i % 7) * 0.01; 2])
            .collect();
        let report = summarize("t", 1, &a, &b);
        assert!(report.low <= 1.0 && report.high >= 1.0, "{report}");
    }

    #[test]
    fn arguments_override_defaults() {
        let args = ["--bench", "--rounds", "40", "--block-ms", "2"].map(String::from);
        let config = Config::from_args(args);
        assert_eq!(config.rounds, 40);
        assert_eq!(config.block, Duration::from_millis(2));
    }
}
