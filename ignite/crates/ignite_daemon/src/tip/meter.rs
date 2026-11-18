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
//!   build joined while another command runs it counts too.
//!
//! When the round ends it sends one [`TipRound`] to the hub, which files an improvement ticket
//! when too much of the round went to anything but checks and builds.

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

/// How many checks a round ran and how many the cache answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Tally {
    pub ran: u32,
    pub cached: u32,
}

/// Counts one round's time. See the module documentation.
pub(super) struct Meter {
    start: Timestamp,
    parts: Mutex<Parts>,
}

impl Meter {
    /// Starts counting now, and returns the meter with the context the round runs with.
    pub(super) fn start(ctx: &Context) -> (Arc<Meter>, Context) {
        let meter = Arc::new(Meter {
            start: ctx.clock.now(),
            parts: Mutex::new(Parts::default()),
        });
        let mut metered = ctx.clone();
        metered.launcher = Arc::new(MeteredLauncher {
            inner: ctx.launcher.clone(),
            meter: meter.clone(),
        });
        metered.builds = Arc::new(MeteredBuilds {
            inner: ctx.builds.clone(),
            clock: ctx.clock.clone(),
            meter: meter.clone(),
        });
        (meter, metered)
    }

    fn add(&self, change: impl FnOnce(&mut Parts)) {
        change(&mut self.parts.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn command(&self, spec: &CommandSpec, usage: &Usage) {
        let runs_checks = spec
            .env
            .iter()
            .any(|(key, _)| key == RESULTS_ENV || key == NEXTEST_JSON_ENV.0);
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
    pub(super) fn finish(&self, ctx: &Context, tip: &Commit, tally: Tally) -> TipRound {
        let now = ctx.clock.now();
        let parts = *self.parts.lock().unwrap_or_else(PoisonError::into_inner);
        TipRound {
            commit: tip.clone(),
            machine: ctx.machine.clone(),
            at: now,
            wall_s: now.seconds_since(self.start).max(0.0),
            ran: tally.ran,
            cached: tally.cached,
            checks_s: parts.checks_s,
            listing_s: parts.listing_s,
            building_s: parts.building_s,
            waiting_s: parts.waiting_s,
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

/// The round's launcher: the daemon's, counting each command.
struct MeteredLauncher {
    inner: Arc<dyn Launcher>,
    meter: Arc<Meter>,
}

impl Launcher for MeteredLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        let result = self.inner.run(spec);
        self.meter.command(spec, &result.usage);
        result
    }

    fn run_until(&self, spec: &CommandSpec, cancelled: &(dyn Fn() -> bool + Sync)) -> RunResult {
        let result = self.inner.run_until(spec, cancelled);
        self.meter.command(spec, &result.usage);
        result
    }
}

/// The round's builds: the daemon's, counting the time until each product is current.
struct MeteredBuilds {
    inner: Arc<dyn Builds>,
    clock: Arc<dyn Clock>,
    meter: Arc<Meter>,
}

impl MeteredBuilds {
    fn timed(&self, build: impl FnOnce() -> BuildResult) -> BuildResult {
        let start = self.clock.now();
        let result = build();
        let seconds = self.clock.now().seconds_since(start).max(0.0);
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
}
