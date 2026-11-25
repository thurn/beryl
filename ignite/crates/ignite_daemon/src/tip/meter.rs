//! Where a tip verification round's time went (see "How long a round takes" in
//! `docs/ignite.md`).
//!
//! A round runs with a [`Context`] whose launcher and builds are wrapped, so every command and
//! every build the round starts, through whichever part of the daemon, is counted:
//!
//! - a command that is given a results file (`IGNITE_RESULTS`) or nextest's JSON output runs
//!   checks, and any other command the round launches lists a family;
//! - the time a command waited for cores is its `queued_s`, apart from the time it ran;
//! - a build counts from when it was asked for until it is current, by the `Clock` trait, so a
//!   build joined while another command runs it counts too;
//! - listing alone is the wall time during which some `list` command was running and no check
//!   command and no build was. Only that part counts towards a slow round, because a listing
//!   that runs beside a build or another family's checks delays nothing. A `list` command
//!   counts from when it is launched, so a stretch it spends waiting for cores while nothing
//!   else of the round runs counts as listing alone too.
//!
//! When the round ends it sends one [`TipRound`] to the hub, which files an improvement ticket
//! when too much of the round went to listing and to Ignite's own work between commands. Time
//! spent waiting for cores is counted and shown but files nothing (see
//! [`TipRound::overhead`]).

use std::sync::{Arc, Mutex, PoisonError};

use ignite_core::ids::{Commit, TreeId};
use ignite_core::protocol::Usage;
use ignite_core::time::Timestamp;
use ignite_core::tipround::TipRound;
use ignite_core::traits::{BuildFor, BuildResult, Builds, Clock, CommandSpec, Launcher, RunResult};

use crate::checks::adapters::{NEXTEST_JSON_ENV, RESULTS_ENV};
use crate::server::Context;

/// What a round counted while it ran, in seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(clippy::struct_field_names)]
struct Parts {
    checks_s: f64,
    listing_s: f64,
    building_s: f64,
    waiting_s: f64,
}

/// How many checks a round ran and how many the cache answered, and how it split its checks by
/// the tip budget.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Tally {
    pub ran: u32,
    pub cached: u32,
    pub budget: Option<ignite_core::tipround::RoundBudget>,
    /// When `master` moved to the tip, if it did.
    pub master_at: Option<Timestamp>,
    /// For each family, how many checks the round left to other machines without building
    /// the family (see `families`). Recorded in the round's budget, when it has one.
    pub elsewhere: std::collections::BTreeMap<String, u32>,
}

/// Which of the round's commands and builds are running, for timing listing alone.
#[derive(Clone, Copy, Debug)]
struct Running {
    listing: u32,
    /// Check commands and builds.
    other: u32,
    /// When either count last changed.
    since: Timestamp,
    /// Seconds during which `listing` was above zero and `other` was zero.
    listing_alone_s: f64,
}

/// What started or ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity {
    Listing,
    Other,
}

/// Counts one round's time. See the module documentation.
pub(super) struct Meter {
    start: Timestamp,
    clock: Arc<dyn Clock>,
    parts: Mutex<Parts>,
    running: Mutex<Running>,
}

impl Meter {
    /// Starts counting now, and returns the meter with the context the round runs with.
    pub(super) fn start(ctx: &Context) -> (Arc<Meter>, Context) {
        let start = ctx.clock.now();
        let meter = Arc::new(Meter {
            start,
            clock: ctx.clock.clone(),
            parts: Mutex::new(Parts::default()),
            running: Mutex::new(Running {
                listing: 0,
                other: 0,
                since: start,
                listing_alone_s: 0.0,
            }),
        });
        let mut metered = ctx.clone();
        metered.launcher = Arc::new(MeteredLauncher {
            inner: ctx.launcher.clone(),
            meter: meter.clone(),
        });
        metered.builds = Arc::new(MeteredBuilds {
            inner: ctx.builds.clone(),
            meter: meter.clone(),
        });
        (meter, metered)
    }

    fn add(&self, change: impl FnOnce(&mut Parts)) {
        change(&mut self.parts.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Records that `activity` started (`+1`) or ended (`-1`) now, first adding the stretch
    /// since the last change to listing alone when only listing was running through it.
    fn shift(&self, activity: Activity, step: i32) {
        let now = self.clock.now();
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.listing > 0 && running.other == 0 {
            running.listing_alone_s += now.seconds_since(running.since).max(0.0);
        }
        let count = match activity {
            Activity::Listing => &mut running.listing,
            Activity::Other => &mut running.other,
        };
        *count = count.saturating_add_signed(step);
        running.since = now;
    }

    /// Runs `work` as one `activity`, counted as running from start to end.
    fn during<T>(&self, activity: Activity, work: impl FnOnce() -> T) -> T {
        self.shift(activity, 1);
        let result = work();
        self.shift(activity, -1);
        result
    }

    fn command(&self, spec: &CommandSpec, usage: &Usage) {
        let runs_checks = runs_checks(spec);
        self.add(|parts| {
            parts.waiting_s += usage.queued_s.max(0.0);
            if runs_checks {
                parts.checks_s += usage.wall_s.max(0.0);
            } else {
                parts.listing_s += usage.wall_s.max(0.0);
            }
        });
    }

    /// The round's record, ended now.
    pub(super) fn finish(&self, ctx: &Context, tip: &Commit, tally: &Tally) -> TipRound {
        // Closes the stretch since the last change, so a listing still running counts.
        self.shift(Activity::Listing, 0);
        let now = ctx.clock.now();
        let parts = *self.parts.lock().unwrap_or_else(PoisonError::into_inner);
        let listing_alone_s = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .listing_alone_s;
        TipRound {
            commit: tip.clone(),
            machine: ctx.machine.clone(),
            at: now,
            wall_s: now.seconds_since(self.start).max(0.0),
            ran: tally.ran,
            cached: tally.cached,
            checks_s: parts.checks_s,
            listing_s: parts.listing_s,
            listing_alone_s: Some(listing_alone_s),
            building_s: parts.building_s,
            waiting_s: parts.waiting_s,
            budget: tally.budget.clone().map(|mut budget| {
                budget.master_s = tally
                    .master_at
                    .map(|moved| moved.seconds_since(self.start).max(0.0));
                budget.elsewhere.clone_from(&tally.elsewhere);
                budget
            }),
        }
    }
}

/// Sends a round's record. The hub may file an improvement ticket for it; a hub that refuses
/// the record is logged and never changes the round's outcome.
pub(super) fn send(ctx: &Context, round: &TipRound) {
    eprintln!("ignite: tip {} round: {}", round.commit, round.summary());
    match ctx.hub.tip_round(round) {
        Ok(filed) => {
            if let (Some(ticket), true) = (filed.ticket, filed.created) {
                eprintln!("ignite: the slow round filed improvement ticket {ticket}");
            }
        }
        Err(error) => eprintln!("ignite: tip round record not sent: {error}"),
    }
}

/// Whether `spec` runs checks; any other command a round launches lists a family.
fn runs_checks(spec: &CommandSpec) -> bool {
    spec.env
        .iter()
        .any(|(key, _)| key == RESULTS_ENV || key == NEXTEST_JSON_ENV.0)
}

fn activity_of(spec: &CommandSpec) -> Activity {
    if runs_checks(spec) {
        Activity::Other
    } else {
        Activity::Listing
    }
}

/// The round's launcher: the daemon's, counting each command.
struct MeteredLauncher {
    inner: Arc<dyn Launcher>,
    meter: Arc<Meter>,
}

impl Launcher for MeteredLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        let result = self
            .meter
            .during(activity_of(spec), || self.inner.run(spec));
        self.meter.command(spec, &result.usage);
        result
    }

    fn run_until(&self, spec: &CommandSpec, cancelled: &(dyn Fn() -> bool + Sync)) -> RunResult {
        let result = self
            .meter
            .during(activity_of(spec), || self.inner.run_until(spec, cancelled));
        self.meter.command(spec, &result.usage);
        result
    }
}

/// The round's builds: the daemon's, counting the time until each product is current.
struct MeteredBuilds {
    inner: Arc<dyn Builds>,
    meter: Arc<Meter>,
}

impl MeteredBuilds {
    fn timed(&self, build: impl FnOnce() -> BuildResult) -> BuildResult {
        let start = self.meter.clock.now();
        let result = self.meter.during(Activity::Other, build);
        let seconds = self.meter.clock.now().seconds_since(start).max(0.0);
        self.meter.add(|parts| parts.building_s += seconds);
        result
    }
}

impl Builds for MeteredBuilds {
    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult {
        self.timed(|| self.inner.ensure(tree, product, packages))
    }

    fn ensure_for(
        &self,
        tree: &TreeId,
        product: &str,
        packages: &[String],
        who: &BuildFor,
    ) -> BuildResult {
        self.timed(|| self.inner.ensure_for(tree, product, packages, who))
    }

    // Nextest must use the same package selection as the build it waits for.
    fn selected(&self, tree: &TreeId, product: &str, packages: &[String]) -> Vec<String> {
        self.inner.selected(tree, product, packages)
    }

    fn has_built(&self, tree: &TreeId, product: &str) -> bool {
        self.inner.has_built(tree, product)
    }

    fn forget(&self, tree: &TreeId) {
        self.inner.forget(tree);
    }

    fn held_in(&self, tree: &TreeId, dirs: &[String]) -> std::collections::BTreeSet<String> {
        self.inner.held_in(tree, dirs)
    }

    fn copy_held(&self, from: &TreeId, to: &TreeId, dirs: &[String]) {
        self.inner.copy_held(from, to, dirs);
    }
}

#[cfg(test)]
mod tests {
    use ignite_core::fakes::{FakeBuilds, FakeClock};

    use super::*;

    fn meter(clock: &Arc<FakeClock>) -> Meter {
        let start = clock.now();
        Meter {
            start,
            clock: clock.clone(),
            parts: Mutex::new(Parts::default()),
            running: Mutex::new(Running {
                listing: 0,
                other: 0,
                since: start,
                listing_alone_s: 0.0,
            }),
        }
    }

    #[test]
    fn tip_nextest_uses_the_builds_package_selection() {
        let tree = TreeId::new("landing-1").unwrap();
        let inner = Arc::new(FakeBuilds::default());
        // The subset's features differ, so the build manager builds the whole workspace.
        inner.script_selected(&tree, "tests", &[]);
        let builds = MeteredBuilds {
            inner,
            meter: Arc::new(meter(&Arc::new(FakeClock::new(Timestamp::EPOCH)))),
        };
        let requested = ["core".to_owned()];

        assert!(builds.selected(&tree, "tests", &requested).is_empty());
    }

    fn alone(meter: &Meter) -> f64 {
        meter.shift(Activity::Listing, 0);
        meter.running.lock().unwrap().listing_alone_s
    }

    /// A family's `list` that runs beside another family's build delays nothing, so only the
    /// stretches with listing and nothing else count.
    #[test]
    fn listing_counts_alone_only_while_nothing_else_runs() {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        let meter = meter(&clock);
        // 0-10 s: one listing alone. 10-70 s: a build starts beside it. 70-80 s: the listing
        // ends at 70 and a second one runs beside the build until 80. 80-100 s: the build ends,
        // the second listing runs alone. 100-130 s: nothing runs.
        meter.shift(Activity::Listing, 1);
        clock.advance(10.0);
        meter.shift(Activity::Other, 1);
        clock.advance(60.0);
        meter.shift(Activity::Listing, -1);
        meter.shift(Activity::Listing, 1);
        clock.advance(10.0);
        meter.shift(Activity::Other, -1);
        clock.advance(20.0);
        meter.shift(Activity::Listing, -1);
        clock.advance(30.0);
        assert!((alone(&meter) - 30.0).abs() < 1e-9);
        // Two listings at once count once.
        meter.shift(Activity::Listing, 1);
        meter.shift(Activity::Listing, 1);
        clock.advance(5.0);
        assert!((alone(&meter) - 35.0).abs() < 1e-9);
    }
}
