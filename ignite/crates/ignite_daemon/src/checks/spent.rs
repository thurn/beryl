//! What the landing gate remembers from one gate to the next on this daemon: the checks each
//! change's last passing gate chose by score, and the rate the gate's check commands achieved
//! lately (see "Where regression checks run" in `docs/ignite.md`).
//!
//! - The budget belongs to the change, not to each command. On 2026-11-19 an `ignite check` of
//!   a one-line comment in a card file ran 1,282 checks in 90 s, all passed, and the
//!   `ignite land` that followed, on the same content, ran 398 more: the checks the check had
//!   chosen were now cached, so they cost nothing, and the land spent a fresh budget on the
//!   next slice of checks with lower scores. A gate now passes the planner the checks the last
//!   passing gate of the same change chose by score ([`GateMemory::paid`]), and those, when
//!   cached, count against its budget again (see `ignite_core::gate`).
//! - A change is known by its own files, the paths that differ from `staging`, sorted. A
//!   rebase over commits that touch other files keeps them, so a landing after `ignite check`
//!   finds what the check chose. A check another change chose is only charged when cached for
//!   this tree, which is harmless, so two changes with the same files need no other key.
//! - The rate that converts the budget's seconds to instructions is the one the gate's check
//!   commands achieved in the gates of the last hour on this daemon ([`GateMemory::rate`]),
//!   when there is one, so a 60 s budget runs in about 60 s under the load of the moment. The
//!   same land spent 469 billion instructions in 173 s at a load average of 12 to 17, which is
//!   2.7 billion a second, against the 7.8 billion of the hub's daily rate.

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Mutex, PoisonError};

use ignite_core::ids::CheckName;
use ignite_core::time::Timestamp;

/// How many changes' chosen checks are remembered; the oldest is forgotten first.
const CHANGES: usize = 64;

/// How long an observed rate stands for the load of the moment, in seconds.
const RATE_FRESH_S: f64 = 3600.0;

/// The least seconds of running checks a gate needs before its rate is believed: the start-up
/// of a single command dominates a shorter one.
const LEAST_OBSERVED_S: f64 = 10.0;

/// How far an observed rate may stray from the hub's daily rate, as factors below and above it,
/// so one odd gate (a check whose median cost is far off its run) cannot shrink the budget to
/// nothing or grow it without bound.
const RATE_RANGE: (f64, f64) = (0.1, 2.0);

/// See the module header.
#[derive(Default)]
pub(crate) struct GateMemory {
    /// For each change, by its sorted own files, the checks its last passing gate chose by
    /// score; the newest last.
    paid: Mutex<VecDeque<(Vec<String>, BTreeSet<CheckName>)>>,
    /// Billions of instructions per second of running checks the last gates achieved, and when.
    rate: Mutex<Option<(Timestamp, f64)>>,
}

impl GateMemory {
    /// The checks the last passing gate of the change whose own files are `changed` chose by
    /// score; none for a change no gate here has passed.
    pub(crate) fn paid(&self, changed: &[String]) -> BTreeSet<CheckName> {
        let key = key(changed);
        let paid = self.paid.lock().unwrap_or_else(PoisonError::into_inner);
        paid.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, checks)| checks.clone())
            .unwrap_or_default()
    }

    /// Remembers `chosen`, the checks a passing gate of the change whose own files are
    /// `changed` chose by score, in place of what an earlier gate of it chose.
    pub(crate) fn passed(&self, changed: &[String], chosen: BTreeSet<CheckName>) {
        let key = key(changed);
        let mut paid = self.paid.lock().unwrap_or_else(PoisonError::into_inner);
        paid.retain(|(k, _)| *k != key);
        paid.push_back((key, chosen));
        while paid.len() > CHANGES {
            paid.pop_front();
        }
    }

    /// The rate the gate converts its seconds at: the one observed in the last hour, kept within
    /// [`RATE_RANGE`] of `daily`, the hub's rate for the day, or `daily` when none was.
    pub(crate) fn rate(&self, now: Timestamp, daily: f64) -> f64 {
        let rate = self.rate.lock().unwrap_or_else(PoisonError::into_inner);
        match *rate {
            Some((at, observed)) if now.seconds_since(at) <= RATE_FRESH_S => {
                observed.clamp(daily * RATE_RANGE.0, daily * RATE_RANGE.1)
            }
            _ => daily,
        }
    }

    /// Records that a gate's check commands spent `billions`, as the planner estimated them from
    /// each check's recorded cost, in `seconds` of running. A fresh earlier rate is averaged
    /// with the new one, so one gate moves the rate halfway.
    pub(crate) fn observe(&self, now: Timestamp, billions: f64, seconds: f64) {
        if seconds < LEAST_OBSERVED_S || !billions.is_finite() || billions <= 0.0 {
            return;
        }
        let sample = billions / seconds;
        let mut rate = self.rate.lock().unwrap_or_else(PoisonError::into_inner);
        let next = match *rate {
            Some((at, earlier)) if now.seconds_since(at) <= RATE_FRESH_S => {
                f64::midpoint(earlier, sample)
            }
            _ => sample,
        };
        *rate = Some((now, next));
    }
}

fn key(changed: &[String]) -> Vec<String> {
    let mut key = changed.to_vec();
    key.sort();
    key.dedup();
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: f64) -> Timestamp {
        Timestamp::parse("2026-11-19T21:00:00Z")
            .unwrap()
            .plus_seconds(seconds)
    }

    fn check(name: &str) -> CheckName {
        CheckName::new(name).unwrap()
    }

    #[test]
    fn a_change_is_known_by_its_own_files_in_any_order() {
        let memory = GateMemory::default();
        let a = ["src/b.rs".to_owned(), "src/a.rs".to_owned()];
        memory.passed(&a, BTreeSet::from([check("unit:x")]));
        let reordered = ["src/a.rs".to_owned(), "src/b.rs".to_owned()];
        assert_eq!(memory.paid(&reordered), BTreeSet::from([check("unit:x")]));
        assert!(memory.paid(&["src/a.rs".to_owned()]).is_empty());
        memory.passed(&a, BTreeSet::from([check("unit:y")]));
        assert_eq!(memory.paid(&a), BTreeSet::from([check("unit:y")]));
    }

    /// The land of the report: 469 billion in 173 s, against a daily rate of 7.8.
    #[test]
    fn the_rate_follows_the_last_hour_of_gates() {
        let memory = GateMemory::default();
        assert!((memory.rate(at(0.0), 7.8) - 7.8).abs() < 1e-9);
        memory.observe(at(0.0), 469.0, 173.0);
        let observed = 469.0 / 173.0;
        assert!((memory.rate(at(60.0), 7.8) - observed).abs() < 1e-9);
        // A second gate moves it halfway to what it achieved.
        memory.observe(at(120.0), 600.0, 100.0);
        let averaged = f64::midpoint(observed, 6.0);
        assert!((memory.rate(at(180.0), 7.8) - averaged).abs() < 1e-9);
        // After an hour with no gate the daily rate stands again.
        assert!((memory.rate(at(120.0 + 3601.0), 7.8) - 7.8).abs() < 1e-9);
    }

    #[test]
    fn a_short_or_odd_gate_does_not_set_the_rate() {
        let memory = GateMemory::default();
        memory.observe(at(0.0), 100.0, 2.0);
        memory.observe(at(0.0), 0.0, 60.0);
        assert!((memory.rate(at(1.0), 7.8) - 7.8).abs() < 1e-9);
        memory.observe(at(0.0), 1.0, 60.0);
        assert!((memory.rate(at(1.0), 7.8) - 0.78).abs() < 1e-9);
        memory.observe(at(4000.0), 6000.0, 60.0);
        assert!((memory.rate(at(4001.0), 7.8) - 15.6).abs() < 1e-9);
    }
}
