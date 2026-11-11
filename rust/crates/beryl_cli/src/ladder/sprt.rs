//! The sequential test: the bars' hypotheses, the pairs it counts and the verdict it reaches
//! (docs/ai-strategy.md §4).

use serde::Serialize;

/// The pre-registered bars of docs/ai-strategy.md §4.2. Each passes when its test accepts
/// `elo1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    /// "No gain" against "+50 Elo" at α = β = 0.001.
    Stronger,
    /// "−25 Elo" against "no change" at α = β = 0.05: not weaker.
    Plausible,
}

impl Bar {
    pub fn name(self) -> &'static str {
        match self {
            Bar::Stronger => "stronger",
            Bar::Plausible => "plausible",
        }
    }

    pub fn sprt(self) -> Sprt {
        match self {
            Bar::Stronger => Sprt {
                elo0: 0.0,
                elo1: 50.0,
                alpha: 0.001,
                beta: 0.001,
            },
            Bar::Plausible => Sprt {
                elo0: -25.0,
                elo1: 0.0,
                alpha: 0.05,
                beta: 0.05,
            },
        }
    }
}

/// A match's seed pairs by the challenger's score over the two games: both won, split (a
/// game each, or any draw or stall), both lost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Pairs {
    pub swept: u32,
    pub split: u32,
    pub lost: u32,
}

impl Pairs {
    pub fn total(self) -> u32 {
        self.swept + self.split + self.lost
    }

    /// Counts a pair by the challenger's half points over its two games, 0 to 4.
    pub fn add(&mut self, halves: u32) {
        match halves {
            4 => self.swept += 1,
            0 => self.lost += 1,
            _ => self.split += 1,
        }
    }
}

/// How a sequential test stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The test accepted `elo1`.
    Pass,
    /// The test accepted `elo0`.
    Fail,
    /// The cap came first.
    Undecided,
}

/// A trinomial sequential probability ratio test over seed pairs, between Elo `elo0` and
/// `elo1`, with error rates `alpha` (accepting `elo1` when `elo0` holds) and `beta`.
///
/// As fishtest's pentanomial test, it reads the pairs' distribution rather than the games',
/// so the correlation between a seed's two games (the same deal, seats swapped) is not taken
/// for evidence. Its hypotheses fix the share of the decided pairs the challenger sweeps,
/// `q = P(swept) / (P(swept) + P(lost))`, and leave the share of splits free. An agent `elo`
/// stronger wins a game with chance `s = 1 / (1 + 10^(-elo/400))`, so with the two games of a
/// pair independent, `q = s² / (s² + (1 - s)²) = 1 / (1 + 10^(-elo/200))`. The generalized
/// log-likelihood ratio of the counts, splits profiled out, is then the binomial ratio of
/// swept against lost: the split share's estimate is the same under both hypotheses and
/// cancels. A split adds nothing to it, which is right for a seed on which the two agents
/// played alike (§4.1), so a run of splits never decides a match. At +50 Elo about half the
/// pairs are decided and the stronger bar ends in about 700 games, as §4.2 expects.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Sprt {
    pub elo0: f64,
    pub elo1: f64,
    pub alpha: f64,
    pub beta: f64,
}

impl Sprt {
    /// Wald's bounds on the log-likelihood ratio: below the first it accepts `elo0`, above the
    /// second `elo1`.
    pub fn bounds(self) -> [f64; 2] {
        [
            (self.beta / (1.0 - self.alpha)).ln(),
            ((1.0 - self.beta) / self.alpha).ln(),
        ]
    }

    /// The log-likelihood ratio of `elo1` to `elo0` given `pairs`.
    pub fn llr(self, pairs: Pairs) -> f64 {
        let (q0, q1) = (swept_share(self.elo0), swept_share(self.elo1));
        f64::from(pairs.swept) * (q1 / q0).ln()
            + f64::from(pairs.lost) * ((1.0 - q1) / (1.0 - q0)).ln()
    }

    /// The test's decision on `pairs`, if it has reached one.
    pub fn decide(self, pairs: Pairs) -> Option<Verdict> {
        let llr = self.llr(pairs);
        let [lower, upper] = self.bounds();
        if llr >= upper {
            Some(Verdict::Pass)
        } else if llr <= lower {
            Some(Verdict::Fail)
        } else {
            None
        }
    }
}

/// The share of decided pairs an agent `elo` stronger sweeps ([`Sprt`]).
pub fn swept_share(elo: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-elo / 200.0))
}

/// The Elo whose [`swept_share`] is `q`.
pub fn elo_of_swept_share(q: f64) -> f64 {
    200.0 * (q / (1.0 - q)).log10()
}

/// The standard deviation of pairs' scores: 1 swept, a half split, 0 lost.
pub fn spread(p: Pairs) -> f64 {
    let n = f64::from(p.total());
    if n == 0.0 {
        return 0.0;
    }
    let mean = (f64::from(p.swept) + 0.5 * f64::from(p.split)) / n;
    let square = (f64::from(p.swept) + 0.25 * f64::from(p.split)) / n;
    (square - mean * mean).max(0.0).sqrt()
}

#[cfg(test)]
mod tests {
    use beryl_engine::rng::SplitMix64;

    use super::*;

    fn pairs(swept: u32, split: u32, lost: u32) -> Pairs {
        Pairs { swept, split, lost }
    }

    #[test]
    fn the_bounds_match_alpha_and_beta() {
        let [lo, hi] = Bar::Stronger.sprt().bounds();
        assert!((lo - (0.001f64 / 0.999).ln()).abs() < 1e-12, "{lo}");
        assert!((hi - (0.999f64 / 0.001).ln()).abs() < 1e-12, "{hi}");
        assert!((hi - 6.906_755).abs() < 1e-6);
        let [lo, hi] = Bar::Plausible.sprt().bounds();
        assert!((lo + 2.944_439).abs() < 1e-6 && (hi - 2.944_439).abs() < 1e-6);
    }

    #[test]
    fn the_swept_share_inverts() {
        assert!((swept_share(0.0) - 0.5).abs() < 1e-12);
        for elo in [-200.0, -25.0, 50.0, 400.0] {
            assert!((elo_of_swept_share(swept_share(elo)) - elo).abs() < 1e-9);
        }
        // +50 Elo wins a game 57.1% of the time and sweeps 64.0% of its decided pairs.
        let s: f64 = 1.0 / (1.0 + 10f64.powf(-50.0 / 400.0));
        let q = s * s / (s * s + (1.0 - s) * (1.0 - s));
        assert!((swept_share(50.0) - q).abs() < 1e-12);
    }

    /// Sweeps alone accept `elo1` at the first pair whose ratio crosses the bound, and losses
    /// alone `elo0`.
    #[test]
    fn sweeps_accept_elo1_within_the_expected_pairs() {
        for bar in [Bar::Stronger, Bar::Plausible] {
            let sprt = bar.sprt();
            let step = (swept_share(sprt.elo1) / swept_share(sprt.elo0)).ln();
            let expected = (sprt.bounds()[1] / step).ceil() as u32;
            for n in 0..expected {
                assert_eq!(sprt.decide(pairs(n, 0, 0)), None, "{bar:?} after {n}");
            }
            assert_eq!(
                sprt.decide(pairs(expected, 0, 0)),
                Some(Verdict::Pass),
                "{bar:?}"
            );
            let step = ((1.0 - swept_share(sprt.elo1)) / (1.0 - swept_share(sprt.elo0))).ln();
            let expected = (sprt.bounds()[0] / step).ceil() as u32;
            assert_eq!(sprt.decide(pairs(0, 0, expected - 1)), None, "{bar:?}");
            assert_eq!(
                sprt.decide(pairs(0, 0, expected)),
                Some(Verdict::Fail),
                "{bar:?}"
            );
        }
        assert_eq!(
            Bar::Stronger.sprt().decide(pairs(27, 0, 0)),
            None,
            "28 sweeps"
        );
        assert_eq!(
            Bar::Stronger.sprt().decide(pairs(28, 0, 0)),
            Some(Verdict::Pass)
        );
    }

    #[test]
    fn splits_never_decide() {
        for bar in [Bar::Stronger, Bar::Plausible] {
            let sprt = bar.sprt();
            assert!(sprt.llr(pairs(0, 1_000_000, 0)).abs() < 1e-12);
            assert_eq!(sprt.decide(pairs(0, 1_000_000, 0)), None);
            // Nor do they move the test on the decided pairs.
            assert!((sprt.llr(pairs(10, 500, 3)) - sprt.llr(pairs(10, 0, 3))).abs() < 1e-12);
        }
    }

    /// Simulated matches at each hypothesis accept the other no more often than the bar's
    /// error rate allows (Wald's bounds are conservative).
    #[test]
    fn simulated_errors_stay_within_alpha_and_beta() {
        let sprt = Bar::Plausible.sprt();
        let mut rng = SplitMix64::new(7);
        let mut wrong = |elo: f64, wrong: Verdict| {
            let q = swept_share(elo);
            let mut errors = 0;
            for _ in 0..2_000 {
                let mut p = Pairs::default();
                let verdict = loop {
                    // Half the pairs split, which changes nothing.
                    if rng.next_u64().is_multiple_of(2) {
                        p.split += 1;
                        continue;
                    }
                    let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                    if u < q {
                        p.swept += 1;
                    } else {
                        p.lost += 1;
                    }
                    if let Some(v) = sprt.decide(p) {
                        break v;
                    }
                };
                errors += u32::from(verdict == wrong);
            }
            f64::from(errors) / 2_000.0
        };
        let alpha = wrong(sprt.elo0, Verdict::Pass);
        let beta = wrong(sprt.elo1, Verdict::Fail);
        assert!(alpha <= 0.06, "alpha {alpha}");
        assert!(beta <= 0.06, "beta {beta}");
    }

    #[test]
    fn the_spread_of_pair_scores() {
        assert!(spread(Pairs::default()).abs() < 1e-12);
        assert!(spread(pairs(0, 9, 0)).abs() < 1e-12);
        assert!((spread(pairs(5, 0, 5)) - 0.5).abs() < 1e-12);
    }
}
