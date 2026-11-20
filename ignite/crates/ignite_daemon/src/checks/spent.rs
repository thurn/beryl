//! What the landing gate remembers from one gate to the next on this daemon: the checks each
//! change's last gate chose by score, and the rate the gate's check commands achieved
//! lately (see "Where regression checks run" in `docs/ignite.md`).
//!
//! - The budget belongs to the change, not to each command. On 2026-11-19 an `ignite check` of
//!   a one-line comment in a card file ran 1,282 checks in 90 s, all passed, and the
//!   `ignite land` that followed, on the same content, ran 398 more: the checks the check had
//!   chosen were now cached, so they cost nothing, and the land spent a fresh budget on the
//!   next slice of checks with lower scores. A gate now passes the planner the checks the last
//!   gate of the same change chose by score ([`GateMemory::paid`]), and those, when cached,
//!   count against its budget again (see `ignite_core::gate`). The choice is remembered when
//!   the gate has planned, before anything runs, so a gate that failed or was cut off by a
//!   restart of the daemon still fixes what the next one spends.
//! - A change is known by its own files, the paths that differ from `staging`, sorted. A
//!   rebase over commits that touch other files keeps them, so a landing after `ignite check`
//!   finds what the check chose. A check another change chose is only charged when cached for
//!   this tree, which is harmless, so two changes with the same files need no other key.
//! - The rate that converts the budget's seconds to instructions is the one the gate's check
//!   commands achieved in the gates of the last hour on this daemon ([`GateMemory::rate`]),
//!   when there is one, so a 60 s budget runs in about 60 s under the load of the moment. The
//!   same land spent 469 billion instructions in 173 s at a load average of 12 to 17, which is
//!   2.7 billion a second, against the 7.8 billion of the hub's daily rate. An older rate
//!   still stands while it is below the daily rate, and the rate counts the seconds the
//!   running stage took, not the sum of its commands' seconds ([`GateMemory::rate`]).
//! - The memory lives in the daemon's database (`ignite_store::local::gate_memory`), not in the
//!   process. `ignite check` and `ignite land` each run in a verb process of their own, so a
//!   memory kept in the process was gone by the next gate: on 2026-11-19 the land of a one-card
//!   comment change after a passing check spent a fresh budget of 468 billion instructions.

use std::collections::BTreeSet;

use rusqlite::Connection;

use ignite_core::ids::CheckName;
use ignite_core::time::Timestamp;
use ignite_store::local::gate_memory;

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

/// See the module header. A failure to read or write the database is printed and taken as
/// nothing remembered: the gate then spends a budget of its own, as it would for a new change.
pub(crate) struct GateMemory<'a>(pub &'a Connection);

impl GateMemory<'_> {
    /// The checks the last gate of the change whose own files are `changed` chose by score; none
    /// for a change no gate here has planned.
    pub(crate) fn paid(&self, changed: &[String]) -> BTreeSet<CheckName> {
        match gate_memory::chosen(self.0, &key(changed)) {
            Ok(checks) => checks
                .iter()
                .flat_map(|checks| checks.lines())
                .filter_map(|name| CheckName::new(name).ok())
                .collect(),
            Err(error) => {
                eprintln!("ignite: the gate's memory cannot be read: {error}");
                BTreeSet::new()
            }
        }
    }

    /// Remembers `chosen`, the checks a gate of the change whose own files are `changed` chose
    /// by score at `now`, in place of what an earlier gate of it chose.
    pub(crate) fn chose(&self, now: Timestamp, changed: &[String], chosen: &BTreeSet<CheckName>) {
        let checks: Vec<String> = chosen.iter().map(ToString::to_string).collect();
        let recorded = gate_memory::record_chosen(
            self.0,
            &key(changed),
            &checks.join("\n"),
            now.millis(),
            CHANGES,
        );
        if let Err(error) = recorded {
            eprintln!("ignite: the gate's memory cannot be written: {error}");
        }
    }

    /// The rate the gate converts its seconds at: the one observed in the last hour, kept within
    /// [`RATE_RANGE`] of `daily`, the hub's rate for the day. An older observed rate still
    /// stands when it is below `daily`, since the machine's load has not been seen to fall:
    /// on 2026-11-19 a gate 65 minutes after the last observed rate of 2.0 billion a second
    /// planned at the daily 5.7 at a load average of 22, and ran four times its budget. With
    /// none observed, `daily` stands.
    pub(crate) fn rate(&self, now: Timestamp, daily: f64) -> f64 {
        match self.last() {
            Some((at, observed)) if now.seconds_since(at) <= RATE_FRESH_S => {
                observed.clamp(daily * RATE_RANGE.0, daily * RATE_RANGE.1)
            }
            Some((_, observed)) => observed.clamp(daily * RATE_RANGE.0, daily),
            None => daily,
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
        let next = self
            .fresh(now)
            .map_or(sample, |earlier| f64::midpoint(earlier, sample));
        if let Err(error) = gate_memory::set_rate(self.0, now.millis(), next) {
            eprintln!("ignite: the gate's rate cannot be written: {error}");
        }
    }

    /// The rate recorded in the last [`RATE_FRESH_S`] seconds before `now`, if one was.
    fn fresh(&self, now: Timestamp) -> Option<f64> {
        self.last()
            .filter(|(at, _)| now.seconds_since(*at) <= RATE_FRESH_S)
            .map(|(_, rate)| rate)
    }

    /// The last rate recorded and when, if one was.
    fn last(&self) -> Option<(Timestamp, f64)> {
        match gate_memory::rate(self.0) {
            Ok(Some((at, rate))) => Some((Timestamp::from_millis(at), rate)),
            Ok(None) => None,
            Err(error) => {
                eprintln!("ignite: the gate's rate cannot be read: {error}");
                None
            }
        }
    }
}

/// The change's own files, sorted, without repeats, one per line.
fn key(changed: &[String]) -> String {
    let mut key = changed.to_vec();
    key.sort();
    key.dedup();
    key.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_store::local::open_in_memory;

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
        let conn = open_in_memory().unwrap();
        let memory = GateMemory(&conn);
        let a = ["src/b.rs".to_owned(), "src/a.rs".to_owned()];
        memory.chose(at(0.0), &a, &BTreeSet::from([check("unit:x")]));
        let reordered = ["src/a.rs".to_owned(), "src/b.rs".to_owned()];
        assert_eq!(memory.paid(&reordered), BTreeSet::from([check("unit:x")]));
        assert!(memory.paid(&["src/a.rs".to_owned()]).is_empty());
        memory.chose(at(1.0), &a, &BTreeSet::from([check("unit:y")]));
        assert_eq!(memory.paid(&a), BTreeSet::from([check("unit:y")]));
    }

    /// The land of the report: 469 billion in 173 s, against a daily rate of 7.8.
    #[test]
    fn the_rate_follows_the_last_hour_of_gates() {
        let conn = open_in_memory().unwrap();
        let memory = GateMemory(&conn);
        assert!((memory.rate(at(0.0), 7.8) - 7.8).abs() < 1e-9);
        memory.observe(at(0.0), 469.0, 173.0);
        let observed = 469.0 / 173.0;
        assert!((memory.rate(at(60.0), 7.8) - observed).abs() < 1e-9);
        // A second gate moves it halfway to what it achieved.
        memory.observe(at(120.0), 600.0, 100.0);
        let averaged = f64::midpoint(observed, 6.0);
        assert!((memory.rate(at(180.0), 7.8) - averaged).abs() < 1e-9);
        // After an hour with no gate, the old rate still stands while it is below the daily
        // rate, since the load has not been seen to fall.
        assert!((memory.rate(at(120.0 + 3601.0), 7.8) - averaged).abs() < 1e-9);
        // A rate above the daily rate stands only for an hour.
        memory.observe(at(8000.0), 6000.0, 600.0);
        assert!((memory.rate(at(8060.0), 7.8) - 10.0).abs() < 1e-9);
        assert!((memory.rate(at(8000.0 + 3601.0), 7.8) - 7.8).abs() < 1e-9);
    }

    #[test]
    fn a_short_or_odd_gate_does_not_set_the_rate() {
        let conn = open_in_memory().unwrap();
        let memory = GateMemory(&conn);
        memory.observe(at(0.0), 100.0, 2.0);
        memory.observe(at(0.0), 0.0, 60.0);
        assert!((memory.rate(at(1.0), 7.8) - 7.8).abs() < 1e-9);
        memory.observe(at(0.0), 1.0, 60.0);
        assert!((memory.rate(at(1.0), 7.8) - 0.78).abs() < 1e-9);
        memory.observe(at(4000.0), 6000.0, 60.0);
        assert!((memory.rate(at(4001.0), 7.8) - 15.6).abs() < 1e-9);
    }

    /// `ignite check` and `ignite land` run in processes of their own, each with its own
    /// connection to the daemon's database: what one gate chose reaches the next.
    #[test]
    fn a_gate_in_another_process_finds_what_the_last_gate_chose() {
        let dir = std::env::temp_dir().join(format!("ignite-spent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("daemon.db");
        let _ = std::fs::remove_file(&path);
        let changed = ["rust/cards/a.rs".to_owned()];
        {
            let first = ignite_store::local::open(&path).unwrap();
            let memory = GateMemory(&first);
            memory.chose(at(0.0), &changed, &BTreeSet::from([check("unit:x")]));
            memory.observe(at(0.0), 469.0, 173.0);
        }
        let land = ignite_store::local::open(&path).unwrap();
        let memory = GateMemory(&land);
        assert_eq!(memory.paid(&changed), BTreeSet::from([check("unit:x")]));
        assert!((memory.rate(at(60.0), 7.8) - 469.0 / 173.0).abs() < 1e-9);
        drop(land);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
