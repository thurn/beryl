//! The order in which one class's waiting requests are served, and how cores are divided among
//! the requests that start together.
//!
//! On 2026-11-20 sixteen to eighteen agent requests waited for jarvis's 15 cores for hours. The
//! scheduler served them in arrival order and shared the free cores evenly, and every build of
//! a class shrank to the class's even share, so each command ran on about one core and took many
//! times its unloaded time. A one-card change's `ignite check` ran 80 minutes without finishing:
//! its one-core listing steps waited behind ten-core builds, and its builds then ran on one
//! core. So within a class:
//!
//! - The smallest request goes first: a listing step that wants one core is not held up by a
//!   build that wants ten. A request that has waited [`AGED_SECONDS`] goes ahead of every request
//!   that has not, in arrival order, so a stream of small requests cannot hold a large one back
//!   for ever.
//! - Each request in that order gets all it asked for before the next gets anything
//!   ([`in_order`]), so the first commands finish at full speed instead of all finishing late.
//! - A shrinking grant gives cores back to its own class only for a request that is small
//!   ([`SMALL_CORES`]) or has waited [`AGED_SECONDS`] ([`Ledger::ranks_ahead_of_grants`]). A
//!   large request that arrived later waits for the build, which runs at full speed, instead of
//!   making every build of the class shrink to one core.

use std::cmp::Ordering;

use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;

use ignite_core::traits::Reserve;

use super::super::GrantRequest;
use super::{Held, Ledger, Waiter};

/// How long a request waits before it goes ahead of every request of its class that has waited
/// less, and before a shrinking grant of its class gives cores back for it whatever its size.
/// Two minutes is the longest wait for cores that a short change should see under full load.
pub const AGED_SECONDS: f64 = 120.0;

/// A request for at most this many cores is small: a listing step, a formatter or a test run of
/// one crate, which ends within seconds to a minute. Shrinking grants of its class give cores
/// back for it at once.
pub const SMALL_CORES: u32 = 2;

impl Ledger {
    /// The waiters of `class` in the order they are served at `now`:
    /// priority requests first, then those that have waited [`AGED_SECONDS`] in arrival order.
    /// The rest go by the cores they want, fewest first, then by arrival order.
    pub(super) fn served_order(&self, class: Class, now: Timestamp) -> Vec<&Waiter> {
        let mut waiters: Vec<&Waiter> = self
            .waiting
            .iter()
            .filter(|w| rank(&w.request) == class)
            .collect();
        // Stable, so equal keys keep the queue's order (see `Ledger::enqueue`).
        waiters.sort_by(|a, b| self.serve_before(a, b, now));
        waiters
    }

    fn serve_before(&self, a: &Waiter, b: &Waiter, now: Timestamp) -> Ordering {
        let aged = |w: &Waiter| aged(w, now);
        let tip_build = |w: &Waiter| w.request.reserve == Some(Reserve::TipBuild);
        if tip_build(a) != tip_build(b) {
            return tip_build(b).cmp(&tip_build(a));
        }
        let priority = |w: &Waiter| w.request.priority_ticket.is_some();
        if priority(a) != priority(b) {
            return priority(b).cmp(&priority(a));
        }
        match (aged(a), aged(b)) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => a.since.cmp(&b.since),
            (false, false) => self
                .want_of(a)
                .cmp(&self.want_of(b))
                .then(a.since.cmp(&b.since)),
        }
    }

    /// Whether `waiter` is one a shrinking grant of its own class gives cores back for: it is
    /// small ([`SMALL_CORES`]) or has waited [`AGED_SECONDS`].
    pub(super) fn ranks_ahead_of_grants(&self, waiter: &Waiter, now: Timestamp) -> bool {
        self.want_of(waiter) <= SMALL_CORES || aged(waiter, now)
    }

    /// The cores `waiter` would take with nothing else waiting.
    pub(super) fn want_of(&self, waiter: &Waiter) -> u32 {
        waiter.request.cores.clamp(1, self.limits.max_per_command())
    }

    /// How long the request of `classes` that has waited longest has waited at `now`, in
    /// seconds; zero when none waits.
    pub fn longest_wait(&self, classes: &[Class], now: Timestamp) -> f64 {
        self.waiting
            .iter()
            .filter(|w| classes.contains(&w.request.class))
            .map(|w| now.seconds_since(w.since).max(0.0))
            .fold(0.0, f64::max)
    }
}

/// The class a request is scheduled in: its own, except that a tip round's build check
/// (`Reserve::TipBuild`) is scheduled in `land`, the highest, and goes first within it.
///
/// On 2026-11-20 tip rounds on one machine took 35 to 90 minutes, most of it building, while up
/// to 15 agent sessions held the machine's cores. A tip build took its reserve at once but waited
/// in `tip`, below `agent`, for the rest of what it asked for, so each round tested a tip that
/// predated the fixes for the failures the round before had reported, and `master` stood still
/// for 11 hours. A tip build decides when `green` moves, where every new ticket starts, so it now
/// takes free cores before landings and agents do, makes their shrinking grants give cores back,
/// and gives none back itself ([`Held::rank`]). It is still reported in `tip`.
pub(super) fn rank(request: &GrantRequest) -> Class {
    if request.reserve == Some(Reserve::TipBuild) {
        Class::Land
    } else {
        request.class
    }
}

impl Held {
    /// The class this grant is scheduled in ([`rank`]).
    pub(super) fn rank(&self) -> Class {
        if self.reserve == Some(Reserve::TipBuild) {
            Class::Land
        } else {
            self.class
        }
    }
}

fn aged(waiter: &Waiter, now: Timestamp) -> bool {
    now.seconds_since(waiter.since) >= AGED_SECONDS
}

/// Gives `cores` to claims that want `wants` each, in order: each gets what it wants, or what is
/// left, before the next gets anything. A claim may get none once the cores run out.
pub(super) fn in_order(wants: &[u32], cores: u32) -> Vec<u32> {
    let mut left = cores;
    wants
        .iter()
        .map(|&want| {
            let given = want.min(left);
            left -= given;
            given
        })
        .collect()
}

/// Shares `cores` among claims that want `wants` each, evenly. A claim that wants less than an
/// even share gets what it wants, and the rest is shared among the others; cores that do not
/// divide evenly go to the earliest claims. A claim may get none when there are more claims
/// than cores. Growing grants of one class share free cores this way.
pub(super) fn share(wants: &[u32], cores: u32) -> Vec<u32> {
    let mut grants = vec![0; wants.len()];
    let mut open: Vec<usize> = (0..wants.len()).collect();
    let mut left = cores;
    while !open.is_empty() {
        let even = left / open.len() as u32;
        let (settled, rest): (Vec<usize>, Vec<usize>) =
            open.iter().partition(|&&i| wants[i] <= even);
        if settled.is_empty() {
            let extra = (left % open.len() as u32) as usize;
            for (n, &i) in open.iter().enumerate() {
                grants[i] = even + u32::from(n < extra);
            }
            break;
        }
        for i in settled {
            grants[i] = wants[i];
            left -= wants[i];
        }
        open = rest;
    }
    grants
}
