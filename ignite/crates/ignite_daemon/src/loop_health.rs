//! What each background loop of the daemon has done lately, so that `ignite status` can show
//! it and `ignite doctor` can say when a loop has stopped making progress or keeps failing.
//!
//! Each loop registers once with [`LoopHealth::register`] and wraps every round of its work
//! in [`LoopTicker::round`]. The daemon's `Parts` holds the [`LoopHealth`]; clones share the
//! same record.
//!
//! Each round is an activity (see [`crate::steps`]) entered on the loop's thread, so what the
//! round does is recorded as it goes, down to the build or the cores it waits for, and a loop
//! given a class with [`LoopTicker::with_class`] runs its builds in that class.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;

use crate::steps::{Activity, Kind, Steps, View, Work};

/// A loop whose last rounds all failed is unhealthy from this many failures in a row.
pub const MAX_CONSECUTIVE_ERRORS: u32 = 20;

/// What is known about one loop.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopReport {
    pub name: String,
    /// The longest time without a finished round that is still healthy.
    pub stall_after_s: f64,
    /// A round has started and not yet ended.
    pub in_round: bool,
    /// When the registration happened, which counts as the end of the round before the first.
    pub registered_at: Timestamp,
    pub last_started: Option<Timestamp>,
    pub last_finished: Option<Timestamp>,
    pub last_error: Option<String>,
    /// Finished rounds, failed ones included.
    pub rounds: u64,
    pub consecutive_errors: u32,
    /// What the round in progress is doing, if one is.
    pub doing: Option<View>,
}

impl LoopReport {
    /// Why the loop is unhealthy at `now`, or `None` if it is healthy.
    pub fn problem(&self, now: Timestamp) -> Option<String> {
        let since = self.last_finished.unwrap_or(self.registered_at);
        // Time the round in progress spent in the scheduler's queue for cores does not count:
        // work of a higher class held them, so the loop was behind, not stuck. On 2026-11-20
        // cygnus's idle round waited 25 minutes for 12 cores during a tip round, then was
        // reported stuck three minutes into its build.
        let behind = match (self.in_round, &self.doing) {
            (true, Some(doing)) => doing.cores_waited_s,
            _ => 0.0,
        };
        let quiet = now.seconds_since(since) - behind;
        if quiet > self.stall_after_s {
            return Some(format!(
                "the {} loop has not finished a round for {} (it should finish one at least \
                 every {}){}",
                self.name,
                span(quiet),
                span(self.stall_after_s),
                match (self.in_round, &self.doing) {
                    (true, Some(doing)) =>
                        format!(", and a round is still running: {}", doing.doing()),
                    (true, None) => ", and a round is still running".to_owned(),
                    (false, _) => String::new(),
                }
            ));
        }
        if self.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
            return Some(format!(
                "the {} loop has failed {} rounds in a row; the last error was: {}",
                self.name,
                self.consecutive_errors,
                self.last_error.as_deref().unwrap_or("(none recorded)")
            ));
        }
        None
    }

    /// The line `ignite status` shows.
    pub fn line(&self, now: Timestamp) -> String {
        let last = self.last_finished.map_or_else(
            || "no round finished yet".to_owned(),
            |at| format!("last round {} ago", span(now.seconds_since(at))),
        );
        let running = match (self.in_round, &self.doing) {
            (true, Some(doing)) => {
                format!(", in a round for {}: {}", span(doing.age_s), doing.doing())
            }
            (true, None) => ", in a round".to_owned(),
            (false, _) => String::new(),
        };
        let errors = match (&self.last_error, self.consecutive_errors) {
            (Some(error), n) if n > 0 => format!("; {n} failed in a row, last: {error}"),
            _ => String::new(),
        };
        format!(
            "{:<15} {} rounds, {last}{running}{errors}",
            self.name, self.rounds
        )
    }
}

impl LoopReport {
    /// Whether a round in progress names what it is doing or waiting for, such as a build or
    /// the cores it waits for. Restarting the daemon would only start that again; what the round
    /// names is what to look at.
    pub fn accounted_for(&self) -> bool {
        self.in_round
            && self
                .doing
                .as_ref()
                .is_some_and(|doing| doing.waiting_for.is_some() || doing.step.is_some())
    }

    /// Whether a round in progress is waiting in the scheduler's queue for cores. Work of a
    /// higher class holds them, so the loop is behind other work, not stuck. The idle loop is
    /// the usual case: its work runs only on cores nothing else wants.
    pub fn waiting_for_cores(&self) -> bool {
        self.in_round
            && self
                .doing
                .as_ref()
                .is_some_and(|doing| doing.waiting_for_cores)
    }
}

/// A duration in the largest unit that keeps it short.
pub(crate) fn span(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    if seconds < 120.0 {
        format!("{seconds:.0} s")
    } else if seconds < 7200.0 {
        format!("{:.0} min", seconds / 60.0)
    } else {
        format!("{:.1} h", seconds / 3600.0)
    }
}

/// The shared record of every loop.
#[derive(Clone)]
pub struct LoopHealth {
    clock: Arc<dyn Clock>,
    daemon_home: Option<PathBuf>,
    loops: Arc<Mutex<BTreeMap<String, LoopReport>>>,
    steps: Arc<Steps>,
    /// The round each loop is in, by loop name.
    rounds: Arc<Mutex<BTreeMap<String, Arc<Activity>>>>,
}

impl LoopHealth {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            steps: Arc::new(Steps::new(clock.clone())),
            clock,
            daemon_home: None,
            loops: Arc::new(Mutex::new(BTreeMap::new())),
            rounds: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Retains failed rounds in the Windows daemon's log for the justiciar monitor.
    pub(crate) fn with_daemon_log(mut self, home: PathBuf) -> Self {
        self.daemon_home = Some(home);
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, LoopReport>> {
        self.loops.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds the loop `name`, which should finish a round at least every `stall_after`.
    pub fn register(&self, name: &str, stall_after: Duration) -> LoopTicker {
        let report = LoopReport {
            name: name.to_owned(),
            stall_after_s: stall_after.as_secs_f64(),
            in_round: false,
            registered_at: self.clock.now(),
            last_started: None,
            last_finished: None,
            last_error: None,
            rounds: 0,
            consecutive_errors: 0,
            doing: None,
        };
        self.lock().insert(name.to_owned(), report);
        LoopTicker {
            health: self.clone(),
            name: name.to_owned(),
            class: None,
        }
    }

    /// The time on the loops' clock.
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// Every loop, by name.
    pub fn reports(&self) -> Vec<LoopReport> {
        let rounds = self
            .rounds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut reports: Vec<LoopReport> = self.lock().values().cloned().collect();
        for report in &mut reports {
            report.doing = rounds
                .get(&report.name)
                .filter(|_| report.in_round)
                .map(|round| round.view());
        }
        reports
    }

    /// The problems with the loops now, one sentence each; empty when all are healthy.
    pub fn problems(&self) -> Vec<String> {
        let now = self.clock.now();
        self.reports()
            .iter()
            .filter_map(|report| report.problem(now))
            .collect()
    }

    /// One line per loop, for `ignite status`.
    pub fn lines(&self) -> Vec<String> {
        let now = self.clock.now();
        self.reports().iter().map(|r| r.line(now)).collect()
    }

    fn update(&self, name: &str, change: impl FnOnce(&mut LoopReport, Timestamp)) -> Timestamp {
        let now = self.clock.now();
        if let Some(report) = self.lock().get_mut(name) {
            change(report, now);
        }
        now
    }
}

/// One loop's handle on its record.
#[derive(Clone)]
pub struct LoopTicker {
    health: LoopHealth,
    name: String,
    class: Option<Class>,
}

impl LoopTicker {
    /// The ticker of a loop whose rounds are work of `class`: the builds and commands they start
    /// without a class of their own run in it (see [`crate::steps::class`]).
    #[must_use]
    pub fn with_class(mut self, class: Class) -> Self {
        self.class = Some(class);
        self
    }

    /// Runs one round of the loop, which says what went wrong as text, and records it.
    pub fn round<T>(&self, work: impl FnOnce() -> Result<T, String>) -> Option<T> {
        let round = self.health.steps.begin(
            Kind::Loop(self.name.clone()),
            Work {
                class: self.class,
                ..Work::default()
            },
        );
        self.health
            .rounds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(self.name.clone(), round.clone());
        self.health.update(&self.name, |report, now| {
            report.in_round = true;
            report.last_started = Some(now);
        });
        let entered = round.enter();
        let outcome = work();
        drop(entered);
        round.finish();
        let finished = self.health.update(&self.name, |report, now| {
            report.in_round = false;
            report.last_finished = Some(now);
            report.rounds += 1;
            match &outcome {
                Ok(_) => report.consecutive_errors = 0,
                Err(error) => {
                    report.consecutive_errors += 1;
                    report.last_error = Some(error.clone());
                }
            }
        });
        if let (Err(error), Some(home)) = (&outcome, &self.health.daemon_home) {
            crate::daemon_log::record(
                home,
                finished,
                &self.name,
                format_args!("round failed: {error}"),
            );
        }
        outcome.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::fakes::FakeClock;

    fn health() -> (Arc<FakeClock>, LoopHealth) {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        (clock.clone(), LoopHealth::new(clock))
    }

    #[test]
    fn a_loop_that_finishes_rounds_is_healthy_and_one_that_goes_quiet_is_not() {
        let (clock, health) = health();
        let ticker = health.register("ignite-claims", Duration::from_secs(600));
        clock.advance(300.0);
        assert!(health.problems().is_empty());
        assert_eq!(ticker.round(|| Ok::<_, String>(7)), Some(7));
        clock.advance(500.0);
        assert!(health.problems().is_empty(), "measured from the last round");
        clock.advance(200.0);
        let problems = health.problems();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("ignite-claims"), "{problems:?}");
    }

    #[test]
    fn a_loop_that_never_finished_a_round_counts_from_its_registration() {
        let (clock, health) = health();
        let ticker = health.register("ignite-tip", Duration::from_secs(10_800));
        clock.advance(11_000.0);
        assert_eq!(health.problems().len(), 1);
        ticker.round(|| Ok::<_, String>(()));
        assert!(health.problems().is_empty());
    }

    #[test]
    fn twenty_failed_rounds_in_a_row_are_a_problem_and_a_success_clears_them() {
        let (clock, health) = health();
        let ticker = health.register("ignite-sender", Duration::from_secs(600));
        for _ in 0..19 {
            ticker.round(|| Err::<(), _>("hub down".to_owned()));
            clock.advance(10.0);
        }
        assert!(health.problems().is_empty());
        ticker.round(|| Err::<(), _>("hub down".to_owned()));
        let problems = health.problems();
        assert!(problems[0].contains("20 rounds in a row"), "{problems:?}");
        assert!(problems[0].contains("hub down"));
        ticker.round(|| Ok::<_, String>(()));
        assert!(health.problems().is_empty());
        let line = &health.lines()[0];
        assert!(line.contains("21 rounds"), "{line}");
    }
}
