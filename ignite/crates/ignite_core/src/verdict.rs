//! Verdicts: what a check's runs add up to, counting the immediate rerun of a failure and the
//! retries of an incomplete run.
//!
//! - A check that fails is run again at once. Passing the second time, on the same input hash,
//!   makes it flaky and not failed. Failing twice makes it failed.
//! - A run that ends without reporting the check is `Incomplete`. It is run again, at most twice
//!   more; a third incomplete attempt gives up, and the hub files a P1 ticket. A check that
//!   failed earlier on the same inputs stays failed instead.
//! - A second flaky record within 7 days of the first, on a different commit, switches the check
//!   off ([`switch_off`]).
//! - A failure counts toward a check's statistics only when a later change made the check pass
//!   again ([`failure_counts`]).

use crate::ids::Commit;
use crate::protocol::Outcome;
use crate::time::Timestamp;

/// Attempts at one input hash that may end incomplete before giving up: the first and two more.
pub const MAX_INCOMPLETE: usize = 3;

/// How long a flaky record stays relevant to switching a check off, in seconds (7 days).
pub const FLAKY_WINDOW_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

/// What to do after a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// The verdict is final.
    Done(Outcome),
    /// Run the check again at once.
    Rerun,
    /// Incomplete too often: the hub files a ticket and anything needing the check waits.
    GiveUp,
}

/// The next step after `runs`, the outcomes so far in order, all on one input hash.
pub fn next(runs: &[Outcome]) -> Next {
    let Some(&last) = runs.last() else {
        return Next::Rerun;
    };
    match last {
        Outcome::Flaky => Next::Done(Outcome::Flaky),
        Outcome::Incomplete => {
            let incomplete = runs.iter().filter(|&&r| r == Outcome::Incomplete).count();
            if incomplete >= MAX_INCOMPLETE && runs.contains(&Outcome::Fail) {
                // It failed on these inputs and nothing later said otherwise.
                Next::Done(Outcome::Fail)
            } else if incomplete >= MAX_INCOMPLETE {
                Next::GiveUp
            } else {
                Next::Rerun
            }
        }
        Outcome::Pass => {
            if runs.contains(&Outcome::Fail) {
                Next::Done(Outcome::Flaky)
            } else {
                Next::Done(Outcome::Pass)
            }
        }
        Outcome::Fail => {
            let failures = runs.iter().filter(|&&r| r == Outcome::Fail).count();
            if failures >= 2 {
                Next::Done(Outcome::Fail)
            } else {
                Next::Rerun
            }
        }
    }
}

/// One recorded flaky result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlakyRecord {
    pub commit: Commit,
    pub at: Timestamp,
}

/// Whether a check is switched off, given its flaky records in any order. Two records on
/// different commits no more than 7 days apart switch it off. The same commit twice is one
/// piece of evidence, not two.
pub fn switch_off(records: &[FlakyRecord]) -> bool {
    records.iter().enumerate().any(|(i, a)| {
        records[i + 1..].iter().any(|b| {
            a.commit != b.commit && {
                let (earlier, later) = if a.at <= b.at { (a, b) } else { (b, a) };
                later.at.seconds_since(earlier.at) <= FLAKY_WINDOW_SECONDS
            }
        })
    })
}

/// Whether a failure counts toward the check's statistics: a later change made the check pass
/// again, so the failure was a fixed bug and not a check that was always wrong. `later` is the
/// check's outcomes after the failure, oldest first.
pub fn failure_counts(later: &[Outcome]) -> bool {
    later.contains(&Outcome::Pass)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Outcome::{Fail, Flaky, Incomplete, Pass};

    fn record(commit: &str, millis: i64) -> FlakyRecord {
        FlakyRecord {
            commit: commit.parse().unwrap(),
            at: Timestamp::from_millis(millis),
        }
    }

    const DAY: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn pass_is_final() {
        assert_eq!(next(&[Pass]), Next::Done(Pass));
    }

    #[test]
    fn first_failure_reruns_then_pass_is_flaky() {
        assert_eq!(next(&[Fail]), Next::Rerun);
        assert_eq!(next(&[Fail, Pass]), Next::Done(Flaky));
    }

    #[test]
    fn two_failures_fail() {
        assert_eq!(next(&[Fail, Fail]), Next::Done(Fail));
    }

    #[test]
    fn incomplete_retries_twice_then_gives_up() {
        assert_eq!(next(&[Incomplete]), Next::Rerun);
        assert_eq!(next(&[Incomplete, Incomplete]), Next::Rerun);
        assert_eq!(next(&[Incomplete, Incomplete, Incomplete]), Next::GiveUp);
    }

    #[test]
    fn a_failure_followed_by_giving_up_is_a_failure() {
        assert_eq!(
            next(&[Fail, Incomplete, Incomplete, Incomplete]),
            Next::Done(Fail)
        );
    }

    #[test]
    fn incomplete_then_pass_is_a_pass() {
        assert_eq!(next(&[Incomplete, Pass]), Next::Done(Pass));
    }

    #[test]
    fn seventh_day_switches_off_and_the_eighth_does_not() {
        let a = record("aaaaaaa", 0);
        assert!(switch_off(&[a.clone(), record("bbbbbbb", 7 * DAY)]));
        assert!(!switch_off(&[a, record("bbbbbbb", 7 * DAY + 1)]));
    }

    #[test]
    fn the_same_commit_twice_does_not_switch_off() {
        assert!(!switch_off(&[record("aaaaaaa", 0), record("aaaaaaa", DAY)]));
    }

    #[test]
    fn one_record_does_not_switch_off_and_order_does_not_matter() {
        assert!(!switch_off(&[record("aaaaaaa", 0)]));
        assert!(switch_off(&[record("bbbbbbb", DAY), record("aaaaaaa", 0)]));
    }

    #[test]
    fn failure_counts_only_after_a_later_pass() {
        assert!(!failure_counts(&[]));
        assert!(!failure_counts(&[Fail, Incomplete]));
        assert!(failure_counts(&[Fail, Pass]));
    }
}
