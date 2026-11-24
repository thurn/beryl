//! Retains a bounded recent trace on the frame thread. Only a capture copies the ring.

use std::collections::VecDeque;
use std::time::Instant;

use beryl_host::timing::{FrameTiming, Outcome, Timing};
use serde::{Deserialize, Serialize};

const SECOND: u64 = 1_000_000_000;
const HISTORY: u64 = 30 * SECOND;
// Room for 30 s of 120 Hz callbacks and their Metal completions, plus pacing events.
const CAPACITY: usize = 8192;
const SLOW_FOR: u64 = 2 * SECOND;
const SLOW_FRACTION: f64 = 0.8;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Slow,
    BugOpened,
    BugSaved,
    Exit,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Window {
    pub reason: Reason,
    pub end_ns: u64,
    pub events: Vec<Timing>,
}

pub(crate) struct Trace {
    epoch: Instant,
    events: VecDeque<Timing>,
    slow: Slow,
    auto_at: Option<u64>,
    pub(crate) frozen: Option<Window>,
}

impl Trace {
    pub(crate) fn new(epoch: Instant) -> Self {
        Self {
            epoch,
            events: VecDeque::with_capacity(CAPACITY),
            slow: Slow::default(),
            auto_at: None,
            frozen: None,
        }
    }

    pub(crate) fn now(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    pub(crate) fn record(&mut self, event: &Timing) -> Option<Window> {
        let now = match event {
            Timing::Frame(frame) => frame.finished_ns,
            _ => event.at_ns(),
        };
        let cutoff = now.saturating_sub(HISTORY);
        while self
            .events
            .front()
            .is_some_and(|event| event.at_ns() < cutoff)
        {
            self.events.pop_front();
        }
        if self.events.len() == CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(*event);
        let slow = match event {
            Timing::Pacer(_) => {
                self.slow = Slow::default();
                false
            }
            Timing::Frame(frame) => self.slow.record(frame),
            _ => false,
        };
        if slow && self.auto_at.is_none_or(|at| now >= at) {
            self.auto_at = Some(now + HISTORY);
            return self.capture(Reason::Slow, now);
        }
        None
    }

    pub(crate) fn capture(&self, reason: Reason, end_ns: u64) -> Option<Window> {
        let cutoff = end_ns.saturating_sub(HISTORY);
        // GPU callbacks and changes can be delivered on either side of the frame record.
        let events: Vec<_> = self
            .events
            .iter()
            .copied()
            .filter(|event| event.at_ns() >= cutoff)
            .collect();
        (!events.is_empty()).then_some(Window {
            reason,
            end_ns,
            events,
        })
    }
}

#[derive(Default)]
struct Slow {
    previous: Option<(u64, u64, f32)>,
    active_ns: u64,
    expected: f64,
    submitted: u32,
}

impl Slow {
    fn record(&mut self, frame: &FrameTiming) -> bool {
        if matches!(frame.outcome, Outcome::Idle | Outcome::Occluded) {
            *self = Self::default();
            return false;
        }
        let epoch = frame.tick.epoch;
        let now = frame.handled_ns;
        let previous = self.previous.replace((epoch, now, frame.tick.requested_hz));
        let Some((previous_epoch, previous_ns, target)) = previous else {
            return false;
        };
        if previous_epoch != epoch {
            self.active_ns = 0;
            self.expected = 0.0;
            self.submitted = 0;
            return false;
        }
        let gap = now.saturating_sub(previous_ns);
        self.active_ns += gap;
        // Integrate the requested rate across intervals so a step within a slow
        // period does not discard the evidence leading up to it.
        self.expected += gap as f64 * f64::from(target) / SECOND as f64;
        self.submitted += u32::from(frame.outcome == Outcome::Submitted);
        if self.active_ns < SLOW_FOR {
            return false;
        }
        let slow = f64::from(self.submitted) < self.expected * SLOW_FRACTION;
        self.active_ns = 0;
        self.expected = 0.0;
        self.submitted = 0;
        slow
    }
}

#[cfg(test)]
mod tests {
    use beryl_host::timing::{DisplayTick, GraphicsTier, PacerState};

    use super::{CAPACITY, HISTORY, Reason, SECOND, Trace};
    use super::{FrameTiming, Instant, Outcome, Timing};

    fn frame(sequence: u64, epoch: u64, at: u64, hz: f32, outcome: Outcome) -> Timing {
        Timing::Frame(FrameTiming {
            tick: DisplayTick {
                sequence,
                epoch,
                requested_hz: hz,
                callback_ns: at,
                display_ns: Some(at as i64 - 1_000_000),
                target_ns: Some(at as i64 + 15_000_000),
                reported_hz: Some(60.0),
            },
            handled_ns: at + 2_000_000,
            finished_ns: at + 3_000_000,
            outcome,
            submitted_ns: (outcome == Outcome::Submitted).then_some(at + 2_500_000),
            cpu: None,
            render_size: [1024, 768],
            fullscreen: false,
            tier: GraphicsTier::Baseline,
            animating: true,
            searching: false,
            gpu_in_flight: None,
            presented_ns: None,
        })
    }

    #[test]
    fn sustained_slow_draws_capture_with_a_cooldown_across_rate_changes() {
        let mut trace = Trace::new(Instant::now());
        let mut captures = Vec::new();
        // One tick per 100 ms, alternating submissions and drawable timeouts.
        for index in 0..400 {
            let hz = if index < 10 { 120.0 } else { 30.0 };
            let outcome = if index % 2 == 0 {
                Outcome::Submitted
            } else {
                Outcome::Timeout
            };
            if let Some(window) = trace.record(&frame(index, 1, index * SECOND / 10, hz, outcome)) {
                captures.push(window);
            }
        }
        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0].reason, Reason::Slow);
        assert_eq!(captures[0].events.len(), 21);
        assert!(captures[1].end_ns - captures[0].end_ns >= HISTORY);
        assert!(captures[1].events.len() <= 301);
    }

    #[test]
    fn deliberate_idle_and_resumes_do_not_count_as_slow() {
        let mut trace = Trace::new(Instant::now());
        for epoch in 1..5 {
            let at = epoch * 4 * SECOND;
            assert!(
                trace
                    .record(&frame(3 * epoch, epoch, at, 60.0, Outcome::Submitted))
                    .is_none()
            );
            assert!(
                trace
                    .record(&frame(
                        3 * epoch + 1,
                        epoch,
                        at + SECOND / 60,
                        60.0,
                        Outcome::Idle
                    ))
                    .is_none()
            );
            let pause = Timing::Pacer(PacerState {
                at_ns: at + SECOND / 60,
                epoch,
                running: false,
                requested_hz: 60.0,
            });
            assert!(trace.record(&pause).is_none());
        }
        assert!(trace.capture(Reason::BugSaved, 23 * SECOND).is_some());
        assert!(trace.capture(Reason::BugSaved, 48 * SECOND).is_none());
    }

    #[test]
    fn the_ring_limits_both_age_and_sample_count() {
        let mut trace = Trace::new(Instant::now());
        for index in 0..=CAPACITY as u64 {
            trace.record(&frame(index, 1, index * 100, 120.0, Outcome::Idle));
        }
        assert_eq!(trace.events.len(), CAPACITY);
        assert!(matches!(trace.events.front(), Some(Timing::Frame(row)) if row.tick.sequence == 1));
        trace.record(&frame(9000, 2, HISTORY + SECOND, 60.0, Outcome::Idle));
        assert_eq!(trace.events.len(), 1);
    }
}
