//! Meta-tickets: templates that the hub copies into a new child ticket on a schedule.
//!
//! A meta-ticket (type `meta`, a root ID starting with `M`) holds the title, text, labels and
//! owner that each copy gets, and `every_hours`, its interval. Copies are due on a fixed clock
//! that starts when the meta-ticket was created: at `created_at`, `created_at + every`,
//! `created_at + 2 * every`, and so on.
//!
//! - At most one copy is live at a time. A due time that passes while a copy is still live is
//!   skipped, so the next copy is filed at the first due time after the live one closes.
//! - Only an `open` meta-ticket is copied. Deferring it pauses the schedule and closing it
//!   retires it; the copies it already filed are left as they are.
//! - The first copy is due as soon as the meta-ticket is created.

use crate::time::Timestamp;

/// What a meta-ticket's schedule knows about its newest copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastCopy {
    /// No copy has been filed yet.
    None,
    /// The newest copy is not closed, so no other copy is filed until it closes.
    Live,
    /// The newest copy closed at this time (or, if it has no close time, was created then).
    ClosedAt(Timestamp),
}

/// When the next copy is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextCopy {
    /// A copy is due now, and the hub files it on its next upkeep pass.
    Now,
    /// The next copy is due at this time.
    At(Timestamp),
    /// The newest copy is still live; the next is due at the first due time after it closes.
    AfterLiveCopy,
}

/// The seconds in `every_hours` hours.
fn interval(every_hours: u32) -> f64 {
    f64::from(every_hours) * 3_600.0
}

/// The most recent due time at or before `now`, or `None` when `now` is before `created_at`.
fn latest_due(created_at: Timestamp, every_hours: u32, now: Timestamp) -> Option<Timestamp> {
    let elapsed = now.seconds_since(created_at);
    if elapsed < 0.0 || every_hours == 0 {
        return None;
    }
    let periods = (elapsed / interval(every_hours)).floor();
    Some(created_at.plus_seconds(periods * interval(every_hours)))
}

/// The time the next copy falls due, whether or not that time has passed: the meta-ticket's
/// creation when it has no copy yet, and the first due time after the newest copy closed
/// otherwise. `None` while a copy is live, or for an interval of 0, which callers refuse.
pub fn due_at(created_at: Timestamp, every_hours: u32, last: LastCopy) -> Option<Timestamp> {
    let closed_at = match last {
        LastCopy::Live => return None,
        LastCopy::None => return Some(created_at),
        LastCopy::ClosedAt(at) => at,
    };
    let after = latest_due(created_at, every_hours, closed_at)?;
    Some(if after > closed_at {
        after
    } else {
        after.plus_seconds(interval(every_hours))
    })
}

/// When a meta-ticket created at `created_at` with an interval of `every_hours` hours next
/// files a copy, given its newest copy.
pub fn next_copy(
    created_at: Timestamp,
    every_hours: u32,
    last: LastCopy,
    now: Timestamp,
) -> NextCopy {
    match due_at(created_at, every_hours, last) {
        None => NextCopy::AfterLiveCopy,
        Some(due) if due <= now => NextCopy::Now,
        Some(due) => NextCopy::At(due),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: f64 = 3_600.0;

    fn at(hours: f64) -> Timestamp {
        Timestamp::EPOCH.plus_seconds(hours * HOUR)
    }

    #[test]
    fn the_first_copy_is_due_at_once() {
        assert_eq!(
            next_copy(at(0.0), 6, LastCopy::None, at(0.0)),
            NextCopy::Now
        );
    }

    #[test]
    fn a_live_copy_holds_the_next_one_back() {
        assert_eq!(
            next_copy(at(0.0), 6, LastCopy::Live, at(30.0)),
            NextCopy::AfterLiveCopy
        );
    }

    #[test]
    fn a_copy_closed_early_waits_for_the_next_due_time() {
        let last = LastCopy::ClosedAt(at(1.0));
        assert_eq!(next_copy(at(0.0), 6, last, at(5.9)), NextCopy::At(at(6.0)));
        assert_eq!(next_copy(at(0.0), 6, last, at(6.0)), NextCopy::Now);
    }

    #[test]
    fn due_times_missed_while_a_copy_was_live_are_skipped() {
        // The copy filed at 0 closed at 14.5, after the due times at 6 and 12 had passed.
        let last = LastCopy::ClosedAt(at(14.5));
        assert_eq!(
            next_copy(at(0.0), 6, last, at(14.5)),
            NextCopy::At(at(18.0))
        );
        assert_eq!(next_copy(at(0.0), 6, last, at(18.2)), NextCopy::Now);
    }

    #[test]
    fn a_copy_closed_exactly_at_a_due_time_waits_a_whole_interval() {
        let last = LastCopy::ClosedAt(at(12.0));
        assert_eq!(
            next_copy(at(0.0), 6, last, at(12.0)),
            NextCopy::At(at(18.0))
        );
    }
}
