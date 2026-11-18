//! The two budgets of regression checks: what a landing's gate may spend before `staging`, and
//! what a tip verification round spends before `master` moves (see "Where regression checks
//! run" in `docs/ignite.md`).
//!
//! - Each budget is stated in seconds of running checks (`[gate] seconds` and `[tip] seconds`
//!   in `ignite.toml`) and spent in instructions. Building is outside both budgets.
//! - Seconds convert to instructions at the rate the class's check commands really achieve on
//!   this machine's average load: the median, over the last [`RATE_DAYS`] days, of each day's
//!   instructions retired per second of wall time by the check commands of that class. The hub
//!   computes it at most once a day (`GET /rates`), so the chosen set does not move with the
//!   load of the moment.
//! - Until the hub has a record for a class, the rates measured on 2026-11-18 stand in:
//!   [`DEFAULT_LAND_RATE`] and [`DEFAULT_TIP_RATE`].

use serde::{Deserialize, Serialize};

/// How many days of records the rate is the median over.
pub const RATE_DAYS: u32 = 7;

/// Billions of instructions per second of wall time that `land`-class check commands retired on
/// jarvis on 2026-11-18 after 06:00 UTC, used until the hub has its own figure.
pub const DEFAULT_LAND_RATE: f64 = 17.5;

/// The same for `tip`-class check commands.
pub const DEFAULT_TIP_RATE: f64 = 39.5;

/// The reply to `GET /rates`: each class's rate in billions of instructions per second of wall
/// time, or `None` when the hub has no record of that class's check commands.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClassRates {
    /// The UTC day (`YYYY-MM-DD`) the rates were computed on.
    pub day: String,
    pub land: Option<f64>,
    pub tip: Option<f64>,
}

impl ClassRates {
    /// The `land` class's rate, or [`DEFAULT_LAND_RATE`].
    pub fn land(&self) -> f64 {
        usable(self.land).unwrap_or(DEFAULT_LAND_RATE)
    }

    /// The `tip` class's rate, or [`DEFAULT_TIP_RATE`].
    pub fn tip(&self) -> f64 {
        usable(self.tip).unwrap_or(DEFAULT_TIP_RATE)
    }
}

fn usable(rate: Option<f64>) -> Option<f64> {
    rate.filter(|rate| rate.is_finite() && *rate > 0.0)
}

/// What checks chosen by score may spend, and the least score worth running.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// Expected failures per billion instructions below which a check is not chosen however
    /// much of the budget is left. Tip verification uses zero.
    pub threshold: f64,
    /// Seconds of running checks.
    pub seconds: f64,
    /// Billions of instructions per second of wall time.
    pub rate: f64,
}

impl Budget {
    /// The budget in billions of instructions.
    pub fn billions(&self) -> f64 {
        self.seconds * self.rate
    }
}

/// `1204.4` billion as `1,204`.
pub fn billions_text(billions: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = billions.max(0.0).round() as u64;
    let digits = whole.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The figures on the ticket that set the two budgets: 60 s of a landing at 17.5 billion a
    /// second is 1,050 billion, and 300 s of a round at 39.5 is 11,850 billion.
    #[test]
    fn the_default_rates_give_the_budgets_the_ticket_names() {
        let rates = ClassRates::default();
        let gate = Budget {
            threshold: 0.0,
            seconds: 60.0,
            rate: rates.land(),
        };
        let tip = Budget {
            threshold: 0.0,
            seconds: 300.0,
            rate: rates.tip(),
        };
        assert_eq!(billions_text(gate.billions()), "1,050");
        assert_eq!(billions_text(tip.billions()), "11,850");
    }

    #[test]
    fn a_rate_the_hub_has_replaces_the_default_and_a_bad_one_does_not() {
        let rates = ClassRates {
            day: "2026-11-19".to_owned(),
            land: Some(20.0),
            tip: Some(f64::NAN),
        };
        assert!((rates.land() - 20.0).abs() < f64::EPSILON);
        assert!((rates.tip() - DEFAULT_TIP_RATE).abs() < f64::EPSILON);
    }
}
