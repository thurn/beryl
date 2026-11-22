//! What the scheduler measures as well as what it grants: the CPU each grant's command really
//! uses, and whether the machine is short of memory.
//!
//! Over an hour on 2026-11-18 every core was granted in every sample, while the machine stayed
//! 50 to 65% idle and requests queued. Ignite's commands used about one core on average: a
//! `uv run` wrapper held almost five cores and used 0.14, and a `cargo nextest list` held one
//! and used 0.15. So a grant whose command uses well below what it holds lends the cores it
//! leaves idle to waiting requests ([`GrantUse::lendable`]), and gets them back as soon as its
//! use rises.

use std::collections::VecDeque;

use ignite_core::time::Timestamp;

use super::Scheduler;

/// How long a grant's use is measured before it lends anything, and the window its long-run
/// use is averaged over. A build alternates between compiling on every core and linking on
/// one, each phase seconds long, so a window shorter than that would lend cores a moment before
/// the command wants them back.
pub const LEND_WINDOW_SECONDS: f64 = 30.0;

/// The window a grant's recent use is averaged over. A grant counts as using the larger of its
/// recent and its long-run use, so it lends slowly, after [`LEND_WINDOW_SECONDS`] of low use,
/// and takes cores back within this window when its use rises. The asymmetry keeps admission
/// from swinging with each reading.
pub const RECENT_SECONDS: f64 = 5.0;

/// Cores kept beside a grant's measured use, so a command that uses 0.2 cores counts as 2 and a
/// small rise in its use takes nothing from another grant.
const LEND_MARGIN: f64 = 1.0;

/// Swap growth within [`SWAP_WINDOW_SECONDS`] beyond which no new work is admitted. The hour of
/// measurement saw about 0.5 GB paged out with swap flat; a quarter of a gigabyte within a
/// minute is the machine starting to page in earnest.
pub const SWAP_GROWTH_MB: u64 = 256;

pub const SWAP_WINDOW_SECONDS: f64 = 60.0;

/// Free memory (free, inactive and purgeable pages) under which no new work is admitted.
pub const LOW_MEMORY_MB: u64 = 1024;

/// The CPU time one grant's command has used, read about once a second.
#[derive(Clone, Debug, Default)]
pub struct GrantUse {
    /// `(when, CPU seconds so far)`, oldest first, covering the last [`LEND_WINDOW_SECONDS`]
    /// and one reading before that.
    readings: VecDeque<(Timestamp, f64)>,
}

impl GrantUse {
    /// Records the command's CPU seconds so far at `now`.
    pub fn record(&mut self, now: Timestamp, cpu_s: f64) {
        if self.readings.back().is_some_and(|&(at, _)| at >= now) {
            return;
        }
        self.readings.push_back((now, cpu_s));
        // Keep one reading at or beyond the window's start, so the window is covered whole.
        while self.readings.len() > 2
            && now.seconds_since(self.readings[1].0) >= LEND_WINDOW_SECONDS
        {
            self.readings.pop_front();
        }
    }

    /// Forgets every reading: the grant's size changed, so its past use says nothing about how
    /// many of its cores it will use now.
    pub fn reset(&mut self) {
        self.readings.clear();
    }

    /// Cores used between the last two readings, or `None` with fewer than two.
    pub fn latest(&self) -> Option<f64> {
        let n = self.readings.len();
        (n >= 2).then(|| rate(self.readings[n - 2], self.readings[n - 1]))
    }

    /// The cores the grant counts as using for admission, at most `cores`: its measured use
    /// plus [`LEND_MARGIN`], rounded up, and at least one. A grant measured for less than
    /// [`LEND_WINDOW_SECONDS`] counts all its cores.
    pub fn lendable(&self, cores: u32) -> u32 {
        let (Some(&first), Some(&last)) = (self.readings.front(), self.readings.back()) else {
            return cores;
        };
        if last.0.seconds_since(first.0) < LEND_WINDOW_SECONDS {
            return cores;
        }
        let recent = self
            .readings
            .iter()
            .rev()
            .find(|(at, _)| last.0.seconds_since(*at) >= RECENT_SECONDS)
            .map_or(0.0, |&reading| rate(reading, last));
        let used = rate(first, last).max(recent);
        let counted = (used + LEND_MARGIN).ceil().max(1.0);
        if counted >= f64::from(cores) {
            cores
        } else {
            counted as u32
        }
    }
}

/// Cores used between two readings. CPU time falls when a process the command started exits
/// without its parent in the tree reaping it, so a fall reads as no use.
fn rate(from: (Timestamp, f64), to: (Timestamp, f64)) -> f64 {
    let seconds = to.0.seconds_since(from.0);
    if seconds <= 0.0 {
        return 0.0;
    }
    ((to.1 - from.1) / seconds).max(0.0)
}

/// Whether the machine is short of memory: swap grew by [`SWAP_GROWTH_MB`] within the last
/// [`SWAP_WINDOW_SECONDS`], or free memory fell under [`LOW_MEMORY_MB`].
#[derive(Clone, Debug, Default)]
pub struct Pressure {
    /// `(when, swap used in MB)`, oldest first, within the last [`SWAP_WINDOW_SECONDS`].
    swap: VecDeque<(Timestamp, u64)>,
}

impl Pressure {
    pub fn record_swap(&mut self, now: Timestamp, used_mb: u64) {
        self.swap.push_back((now, used_mb));
        while self
            .swap
            .front()
            .is_some_and(|&(at, _)| now.seconds_since(at) > SWAP_WINDOW_SECONDS)
        {
            self.swap.pop_front();
        }
    }

    /// Swap used at the last reading.
    pub fn swap_used_mb(&self) -> Option<u64> {
        self.swap.back().map(|&(_, mb)| mb)
    }

    /// How much swap grew within the window: the last reading less the smallest before it.
    pub fn swap_growth_mb(&self) -> u64 {
        let Some(&(_, last)) = self.swap.back() else {
            return 0;
        };
        let smallest = self.swap.iter().map(|&(_, mb)| mb).min().unwrap_or(last);
        last.saturating_sub(smallest)
    }

    pub fn pressed(&self, free_memory_mb: Option<u64>) -> bool {
        self.swap_growth_mb() >= SWAP_GROWTH_MB
            || free_memory_mb.is_some_and(|mb| mb < LOW_MEMORY_MB)
    }
}

impl Scheduler {
    /// Reads current OS pressure for the heartbeat and worker admission.
    pub(crate) fn active_memory_pressure(&self) -> Option<bool> {
        self.host.active_memory_pressure().ok()
    }

    /// Reads the file event daemon's RSS for the heartbeat without affecting admission.
    pub(crate) fn fseventsd_rss_mib(&self) -> Option<u64> {
        self.host.fseventsd_rss_mib().ok()
    }

    /// Reads memory for a worker admission decision without waiting for the next sample.
    /// The OS may report pressure with reclaimable memory available and steady swap use.
    /// When the OS pressure reading is unavailable, the scheduler's memory readings still apply.
    pub fn worker_memory(&self) -> (bool, Option<u64>) {
        let free = self.host.free_memory_mb().ok();
        let pressed =
            self.active_memory_pressure() == Some(true) || self.lock().ledger.memory_pressed(free);
        (pressed, free)
    }
}
