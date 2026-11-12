//! A match to play and its result, as `match.json` and `docs/ladder.md` keep it.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Serialize;

use super::sprt::{Bar, Pairs, Sprt, Verdict, elo_of_swept_share, spread};
use crate::positions::Position;
use crate::tournament::wilson;

/// A match to play.
#[derive(Clone, Debug)]
pub struct Match {
    pub challenger: String,
    pub champion: String,
    /// The `--work` budget the names were resolved at, for the record.
    pub work: u64,
    pub bar: Bar,
    /// The most games played.
    pub cap: u32,
    pub seed: u64,
    pub out: PathBuf,
    /// The start positions, by file name; none to start every pair from the shuffle.
    pub positions: Vec<(String, Position)>,
}

impl Match {
    /// The position pair `pair` starts from: four pairs in five, cycling through the set so
    /// each is used once before any twice; none for the fifth, or without a set.
    pub fn position(&self, pair: u32) -> Option<&(String, Position)> {
        if self.positions.is_empty() || pair % 5 == 4 {
            return None;
        }
        let k = (pair - pair / 5) as usize;
        self.positions.get(k % self.positions.len())
    }
}

/// A match's result, as `match.json` keeps it.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub date: String,
    pub challenger: String,
    pub champion: String,
    pub budget: String,
    pub bar: &'static str,
    pub sprt: Option<Sprt>,
    pub seed: u64,
    pub cap: u32,
    pub games: u32,
    pub pairs: Pairs,
    /// The pairs by how they started, and the spread (standard deviation) of their pair
    /// scores, 1 swept, a half split, 0 lost: what the positions buy (§4.1).
    pub from_positions: Pairs,
    pub from_shuffle: Pairs,
    pub spread_positions: f64,
    pub spread_shuffle: f64,
    /// The challenger's games won, lost and drawn (a stall counts as a draw).
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    /// The challenger's share of the points, and its Wilson 95% interval.
    pub score: f64,
    pub score_interval: [f64; 2],
    /// The challenger's Elo over the champion from the decided pairs, and its 95% interval.
    pub elo: f64,
    pub elo_interval: [f64; 2],
    pub llr: Option<f64>,
    pub bounds: Option<[f64; 2]>,
    pub verdict: Verdict,
    pub out: String,
}

impl Report {
    /// Fills in the statistics from the counts.
    pub(super) fn tally(&mut self) {
        let points = f64::from(self.wins) + 0.5 * f64::from(self.draws);
        self.score = points / f64::from(self.games.max(1));
        self.score_interval = wilson(points, self.games);
        let decided = self.pairs.swept + self.pairs.lost;
        // Half a pair each way keeps the estimate finite on a clean sweep.
        let q = (f64::from(self.pairs.swept) + 0.5) / (f64::from(decided) + 1.0);
        self.elo = elo_of_swept_share(q);
        let [lo, hi] = wilson(f64::from(self.pairs.swept), decided);
        self.elo_interval = [elo_of_swept_share(lo), elo_of_swept_share(hi)];
        self.llr = self.sprt.map(|test| test.llr(self.pairs));
        self.bounds = self.sprt.map(Sprt::bounds);
        self.spread_positions = spread(self.from_positions);
        self.spread_shuffle = spread(self.from_shuffle);
    }

    /// The match as a `docs/ladder.md` row, under [`ROW_HEADER`]. The suite's open and fixed
    /// counts and the human games since the last promotion (§4.3, §5) are left as `?` for the
    /// hand that adds the row.
    pub fn row(&self) -> String {
        format!(
            "| {} | {} | {} | {} | {} | {:.1}% | {} [{}, {}] | {} | ? / ? | ? |",
            self.date,
            self.challenger,
            self.champion,
            self.budget,
            self.games,
            100.0 * self.score,
            signed(self.elo),
            signed(self.elo_interval[0]),
            signed(self.elo_interval[1]),
            match self.verdict {
                Verdict::Pass => "pass",
                Verdict::Fail => "fail",
                Verdict::Undecided => "undecided",
                Verdict::Audit => "audit",
            },
        )
    }

    /// The match as text, ending with its row.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "{} v {} at {}, bar {}",
            self.challenger, self.champion, self.budget, self.bar
        );
        if let Some(test) = self.sprt {
            let _ = write!(
                out,
                " (elo {} v {}, alpha {}, beta {})",
                test.elo0, test.elo1, test.alpha, test.beta
            );
        }
        let _ = writeln!(out, ", seed {}", self.seed);
        let _ = writeln!(
            out,
            "games {} (cap {}): {} won, {} lost, {} drawn",
            self.games, self.cap, self.wins, self.losses, self.draws
        );
        let _ = writeln!(
            out,
            "pairs {}: {} swept, {} split, {} lost",
            self.pairs.total(),
            self.pairs.swept,
            self.pairs.split,
            self.pairs.lost
        );
        if self.from_positions.total() > 0 {
            let _ = writeln!(
                out,
                "from positions {}: {} swept, {} split, {} lost, spread {:.3}; from the shuffle \
                 {}: {} swept, {} split, {} lost, spread {:.3}",
                self.from_positions.total(),
                self.from_positions.swept,
                self.from_positions.split,
                self.from_positions.lost,
                self.spread_positions,
                self.from_shuffle.total(),
                self.from_shuffle.swept,
                self.from_shuffle.split,
                self.from_shuffle.lost,
                self.spread_shuffle
            );
        }
        let _ = writeln!(
            out,
            "score {:.1}% [{:.1}, {:.1}]; Elo {} [{}, {}]",
            100.0 * self.score,
            100.0 * self.score_interval[0],
            100.0 * self.score_interval[1],
            signed(self.elo),
            signed(self.elo_interval[0]),
            signed(self.elo_interval[1])
        );
        if self.verdict == Verdict::Audit {
            let result = if self.score_interval[1] >= 0.5 {
                "passed"
            } else {
                "failed"
            };
            let _ = writeln!(out, "audit: {result}");
        } else if let (Some(llr), Some([lo, hi])) = (self.llr, self.bounds) {
            let _ = writeln!(
                out,
                "LLR {llr:.2} in [{lo:.2}, {hi:.2}]: {:?}",
                self.verdict
            );
        }
        let _ = writeln!(out, "games in {}\n\n{ROW_HEADER}\n{}", self.out, self.row());
        out
    }
}

/// The header of `docs/ladder.md`'s table.
pub const ROW_HEADER: &str = "| Date | Challenger | Champion | Budget | Games | Score | Elo (95%) \
                              | Verdict | Suite open / fixed | Human games |\n\
                              | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |";

/// An Elo figure with its sign, whole.
fn signed(elo: f64) -> String {
    if elo.is_finite() {
        format!("{elo:+.0}")
    } else if elo > 0.0 {
        "+inf".to_owned()
    } else {
        "-inf".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(swept: u32, split: u32, lost: u32) -> Pairs {
        Pairs { swept, split, lost }
    }

    #[test]
    fn the_row_renders() {
        let mut r = Report {
            date: "2026-11-08".to_owned(),
            challenger: "pimc72/hidden/rollout/tuned/anchor+mid@3M".to_owned(),
            champion: "pimc72/hidden/rollout/tuned/anchor@3M".to_owned(),
            budget: "3M".to_owned(),
            bar: "stronger",
            sprt: Bar::Stronger.sprt(),
            seed: 1,
            cap: 2_000,
            games: 700,
            pairs: pairs(120, 162, 68),
            from_positions: Pairs::default(),
            from_shuffle: pairs(120, 162, 68),
            spread_positions: 0.0,
            spread_shuffle: 0.0,
            wins: 400,
            losses: 290,
            draws: 10,
            score: 0.0,
            score_interval: [0.0, 1.0],
            elo: 0.0,
            elo_interval: [0.0, 0.0],
            llr: None,
            bounds: None,
            verdict: Verdict::Pass,
            out: "/tmp/x".to_owned(),
        };
        r.tally();
        assert_eq!(
            r.row(),
            "| 2026-11-08 | pimc72/hidden/rollout/tuned/anchor+mid@3M | \
             pimc72/hidden/rollout/tuned/anchor@3M | 3M | 700 | 57.9% | +49 [+24, +75] | pass | \
             ? / ? | ? |"
        );
        let columns = |s: &str| s.matches('|').count();
        assert_eq!(
            columns(&r.row()),
            columns(ROW_HEADER.lines().next().unwrap())
        );
        assert!(r.text().ends_with(&format!("{}\n", r.row())));
    }
}
