//! Exploration shares: how idle time is split between exploration families by their yield.
//!
//! ```text
//! yield(family) = (bugs found in 30 days + 1) / instructions spent in those days
//! share(family) = yield(family) / sum of every family's yield
//! ```
//!
//! A share under 5% is raised to 5% and the others are scaled down to make room, which keeps a
//! family measured after a quiet month.

/// The smallest share a family keeps.
pub const MIN_SHARE: f64 = 0.05;

/// One exploration family's last 30 days.
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyYield {
    pub family: String,
    pub bugs: u32,
    /// Billions of instructions.
    pub instructions: f64,
}

impl FamilyYield {
    /// Bugs found plus one, per billion instructions. A family that spent nothing has the
    /// yield of one that spent the smallest measurable amount, so it is tried first.
    fn rate(&self) -> f64 {
        f64::from(self.bugs + 1) / self.instructions.max(f64::MIN_POSITIVE)
    }
}

/// Each family's share of idle time, summing to 1, in the order given.
pub fn shares(families: &[FamilyYield]) -> Vec<(String, f64)> {
    if families.is_empty() {
        return Vec::new();
    }
    let rates: Vec<f64> = families.iter().map(FamilyYield::rate).collect();
    let total: f64 = rates.iter().sum();
    let mut shares: Vec<f64> = rates
        .iter()
        .map(|rate| if total.is_finite() { rate / total } else { 0.0 })
        .collect();
    if !total.is_finite() {
        // A rate overflowed: split among the families that did.
        let infinite = rates.iter().filter(|r| r.is_infinite()).count() as f64;
        for (share, rate) in shares.iter_mut().zip(&rates) {
            *share = if rate.is_infinite() {
                1.0 / infinite
            } else {
                0.0
            };
        }
    }
    // Raise each share under the floor to it and scale the others down. Raising can push
    // another share under the floor, so repeat until none changes.
    let floor = MIN_SHARE.min(1.0 / families.len() as f64);
    let mut raised = vec![false; shares.len()];
    loop {
        let fixed = raised.iter().filter(|&&r| r).count() as f64 * floor;
        let free: f64 = shares
            .iter()
            .zip(&raised)
            .filter(|&(_, &r)| !r)
            .map(|(s, _)| s)
            .sum();
        let scale = if free > 0.0 {
            (1.0 - fixed) / free
        } else {
            0.0
        };
        let mut changed = false;
        for (share, raised) in shares.iter_mut().zip(raised.iter_mut()) {
            if *raised {
                *share = floor;
            } else {
                *share *= scale;
                if *share < floor {
                    *raised = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
        // Restart from the unscaled shares for the families still free.
        for (share, (rate, raised)) in shares.iter_mut().zip(rates.iter().zip(&raised)) {
            if !*raised {
                *share = rate / total;
            }
        }
    }
    families
        .iter()
        .map(|f| f.family.clone())
        .zip(shares)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family(name: &str, bugs: u32, instructions: f64) -> FamilyYield {
        FamilyYield {
            family: name.to_owned(),
            bugs,
            instructions,
        }
    }

    fn sum(shares: &[(String, f64)]) -> f64 {
        shares.iter().map(|(_, s)| s).sum()
    }

    #[test]
    fn shares_are_proportional_to_yield_and_sum_to_one() {
        // Yields 2/1 and 1/1 give 2/3 and 1/3.
        let result = shares(&[family("a", 1, 1.0), family("b", 0, 1.0)]);
        assert!((result[0].1 - 2.0 / 3.0).abs() < 1e-12);
        assert!((result[1].1 - 1.0 / 3.0).abs() < 1e-12);
        assert!((sum(&result) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_family_at_zero_bugs_keeps_five_percent() {
        // Yields 1000/1 and 1/1000 would give the second about 0.
        let result = shares(&[family("rich", 999, 1.0), family("quiet", 0, 1000.0)]);
        assert!((result[1].1 - 0.05).abs() < 1e-12);
        assert!((result[0].1 - 0.95).abs() < 1e-12);
        assert!((sum(&result) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn raising_one_family_can_push_another_under_the_floor() {
        let result = shares(&[
            family("big", 99, 1.0),
            family("small", 0, 100.0),
            family("middling", 0, 25.0),
        ]);
        assert!(result.iter().all(|(_, s)| *s >= 0.05 - 1e-12));
        assert!((sum(&result) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn one_family_takes_everything_and_none_gives_nothing() {
        assert_eq!(
            shares(&[family("only", 0, 5.0)]),
            vec![("only".to_owned(), 1.0)]
        );
        assert!(shares(&[]).is_empty());
    }

    #[test]
    fn a_family_that_spent_nothing_still_gives_shares_summing_to_one() {
        let result = shares(&[family("new", 0, 0.0), family("old", 3, 10.0)]);
        assert!((sum(&result) - 1.0).abs() < 1e-12);
        assert!(result[0].1 > result[1].1);
    }
}
