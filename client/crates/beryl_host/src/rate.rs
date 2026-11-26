//! The frame rate a host asks the display for, and when it steps down or back up (decision C18
//! in docs/beryl-3d.md: the highest rate the display and device can hold, but smooth motion
//! comes first). Every host feeds this rule the time between its animating frames and carries
//! out the steps it returns: the macOS display link changes its preferred frame rate range, and
//! the phone hosts will change `CADisplayLink`'s and `Surface.setFrameRate`'s.
//!
//! A step down helps only when frames are late most of the time because drawing them does not
//! fit the interval: an even lower rate then beats an uneven higher one. It does not help a
//! tick that arrives late after a short frame, and on the development Mac's 120 Hz display a
//! lower rate adds late frames of its own.
//!
//! - **Settling.** The first `settle_frames` animating frames after the host opens are not
//!   counted: the first second of a new window was often late while the system set it up.
//! - **A frame misses the rate** when more than `late_intervals` of the chosen rate's
//!   intervals passed since the frame before it, while the ticks ran.
//! - **A miss is explained** when the frame before it spent more than `room` of the interval
//!   from the start of its tick to the return of its present call: the client's frame, the
//!   wait for a drawable, encoding and presenting. A slow GPU is explained, because its frames
//!   wait for a drawable, and so is a display that the window server holds to a lower cadence.
//!   A tick that came late after a short frame is not.
//! - **Stepping down.** When `misses` of the last `window` animating frames were explained
//!   misses, the rule first drops the Enhanced tier to Baseline, if the tier is chosen
//!   automatically and is Enhanced. If frames still miss, it asks for the next rate that
//!   divides the display's (120 to 60 to 40 to 30 Hz), down to `lowest_hz`.
//! - **Stepping back up.** After a run of `up_frames` animating frames in which at most
//!   `up_misses` missed the rate or had work (the client's frame, encoding and presenting,
//!   without the wait for a drawable) over `room` of the higher rate's interval, it asks for
//!   the next rate up. More than `up_misses` such frames restart the run.
//! - **Not going back and forth.** When the rule steps down within `up_frames` frames of a
//!   step up, the step up failed, and the run the next one needs doubles, up to
//!   `up_frames_most`. A device whose window server cannot hold the higher rate therefore tries
//!   it less and less often instead of alternating. A single late frame after a step up is not
//!   a failure.
//!
//! The thresholds are in `data/client/frame-rate.json`.

pub use kit_base::data::playback::FrameRate as Thresholds;

/// What the rule needs to know of one animating frame, in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paced {
    /// The time since the frame before it, while the ticks ran.
    pub gap: u64,
    /// The app's own work for the frame: the client's frame, encoding its GPU commands and
    /// presenting, without the wait for a drawable.
    pub work: u64,
    /// The frame before it, from the start of its tick to the return of its present call:
    /// its work and its wait for a drawable. A miss counts toward a step down only when this
    /// is over `room` of the interval.
    pub previous: u64,
}

/// A change the host must make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// Draw in the Baseline tier for the rest of the session (decision C8).
    Baseline,
    /// Ask the display for this rate, in frames a second.
    Rate(f32),
}

/// The rule's input and counters at a step, before they are reset for the new rate.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Decision {
    /// The frame that caused the step: its gap, its work and the frame before it.
    pub gap_ns: u64,
    pub work_ns: u64,
    pub previous_ns: u64,
    /// Explained misses in the last `window` frames, and how many step down.
    pub missed: usize,
    pub window: usize,
    pub misses_needed: usize,
    pub late_intervals: f32,
    pub room_fraction: f32,
    /// The run toward a step up: its length, the frames in it that were late or lacked room,
    /// how many of those it may hold, and the length a step up needs.
    pub run: usize,
    pub run_bad: usize,
    pub up_misses: usize,
    pub run_needed: usize,
}

/// The rule's state: the rate chosen, and what the last frames did.
#[derive(Clone, Debug)]
pub struct RateRule {
    thresholds: Thresholds,
    display_hz: f32,
    /// The display's rate is divided by this to give the chosen rate.
    divisor: usize,
    /// How many more frames are not counted while the window settles.
    settling: usize,
    /// Whether the tier may still drop to Baseline: it is Enhanced and chosen automatically.
    tier_can_drop: bool,
    /// Whether each of the last `window` frames was an explained miss, as a ring, and how many
    /// were.
    missed: Vec<bool>,
    next: usize,
    missed_count: usize,
    /// The run toward a step up, and the frames in it that were late or lacked room.
    run: usize,
    run_bad: usize,
    /// The run the next step up needs.
    up_frames: usize,
    /// Frames since the last step up, while a step down would still make it a failure.
    since_up: Option<usize>,
    decision: Option<Decision>,
}

impl RateRule {
    /// The rule for a display of `display_hz`, starting at that rate. `tier_can_drop` says
    /// whether the tier is Enhanced and chosen automatically.
    #[must_use]
    pub fn new(display_hz: f32, tier_can_drop: bool, thresholds: Thresholds) -> Self {
        Self {
            thresholds,
            display_hz,
            divisor: 1,
            settling: thresholds.settle_frames,
            tier_can_drop,
            missed: vec![false; thresholds.window.max(1)],
            next: 0,
            missed_count: 0,
            run: 0,
            run_bad: 0,
            up_frames: thresholds.up_frames,
            since_up: None,
            decision: None,
        }
    }

    /// Allows or forbids the tier's drop from now on, after the player changed how the tier is
    /// chosen.
    pub fn set_tier_can_drop(&mut self, tier_can_drop: bool) {
        self.tier_can_drop = tier_can_drop;
    }

    /// The rate chosen, in frames a second.
    #[must_use]
    pub fn hz(&self) -> f32 {
        self.display_hz / self.divisor as f32
    }

    /// The display's highest rate, in frames a second.
    #[must_use]
    pub fn display_hz(&self) -> f32 {
        self.display_hz
    }

    /// The chosen rate's interval, in nanoseconds.
    #[must_use]
    pub fn interval(&self) -> f64 {
        1e9 / f64::from(self.hz())
    }

    /// Whether a frame `gap` nanoseconds after the one before it missed the chosen rate.
    #[must_use]
    pub fn misses(&self, gap: u64) -> bool {
        gap as f64 > f64::from(self.thresholds.late_intervals) * self.interval()
    }

    /// Whether a frame that took `previous` nanoseconds from its tick to its present call
    /// explains a miss of the frame after it.
    fn explains(&self, previous: u64) -> bool {
        previous as f64 > f64::from(self.thresholds.room) * self.interval()
    }

    /// Counts an animating frame, and returns the step the host must take, if any.
    pub fn frame(&mut self, paced: Paced) -> Option<Step> {
        if self.settling > 0 {
            self.settling -= 1;
            return None;
        }
        let missed = self.misses(paced.gap);
        let explained = missed && self.explains(paced.previous);
        if let Some(since) = &mut self.since_up {
            *since += 1;
            if *since >= self.thresholds.up_frames {
                // The step up held: the next one needs only the ordinary run.
                self.up_frames = self.thresholds.up_frames;
                self.since_up = None;
            }
        }
        if self.missed[self.next] {
            self.missed_count -= 1;
        }
        self.missed[self.next] = explained;
        self.next = (self.next + 1) % self.missed.len();
        if explained {
            self.missed_count += 1;
        }
        if self.missed_count >= self.thresholds.misses
            && let Some(step) = self.step_down(paced)
        {
            return Some(step);
        }
        if self.divisor <= 1 {
            return None;
        }
        self.run += 1;
        if missed || !self.has_room(paced.work) {
            self.run_bad += 1;
        }
        if self.run_bad > self.thresholds.up_misses {
            self.run = 0;
            self.run_bad = 0;
        } else if self.run >= self.up_frames {
            self.decision = Some(self.measurements(paced));
            self.divisor -= 1;
            self.restart();
            self.since_up = Some(0);
            return Some(Step::Rate(self.hz()));
        }
        None
    }

    /// The measurements for the most recent step, if there has been one.
    #[must_use]
    pub fn decision(&self) -> Option<Decision> {
        self.decision
    }

    fn measurements(&self, paced: Paced) -> Decision {
        Decision {
            gap_ns: paced.gap,
            work_ns: paced.work,
            previous_ns: paced.previous,
            missed: self.missed_count,
            window: self.missed.len(),
            misses_needed: self.thresholds.misses,
            late_intervals: self.thresholds.late_intervals,
            room_fraction: self.thresholds.room,
            run: self.run,
            run_bad: self.run_bad,
            up_misses: self.thresholds.up_misses,
            run_needed: self.up_frames,
        }
    }

    /// The tier, or else the rate, one step lower; `None` at the lowest rate.
    fn step_down(&mut self, paced: Paced) -> Option<Step> {
        if self.tier_can_drop {
            self.decision = Some(self.measurements(paced));
            self.tier_can_drop = false;
            self.restart();
            return Some(Step::Baseline);
        }
        let lower = self.display_hz / (self.divisor + 1) as f32;
        if lower < self.thresholds.lowest_hz as f32 - 0.5 {
            return None;
        }
        self.decision = Some(self.measurements(paced));
        if self.since_up.take().is_some() {
            // A step down soon after a step up: the step up failed, and the next one waits
            // twice as long.
            self.up_frames = (self.up_frames * 2).min(self.thresholds.up_frames_most);
        }
        self.divisor += 1;
        self.restart();
        Some(Step::Rate(self.hz()))
    }

    /// Whether a frame whose own work took `work` nanoseconds would fit the next rate up.
    fn has_room(&self, work: u64) -> bool {
        if self.divisor <= 1 {
            return false;
        }
        let higher = 1e9 * (self.divisor - 1) as f64 / f64::from(self.display_hz);
        work as f64 <= f64::from(self.thresholds.room) * higher
    }

    /// Forgets the frames before a step, which were shown at the old rate.
    fn restart(&mut self) {
        self.missed.fill(false);
        self.missed_count = 0;
        self.run = 0;
        self.run_bad = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The thresholds in `data/client/frame-rate.json`, with no settling frames.
    const THRESHOLDS: Thresholds = Thresholds {
        settle_frames: 0,
        late_intervals: 1.5,
        window: 240,
        misses: 60,
        room: 0.5,
        up_frames: 600,
        up_misses: 30,
        up_frames_most: 9600,
        lowest_hz: 30,
    };

    /// A frame arriving at `hz`, with `work_ms` of the app's own work, after a frame that took
    /// `previous_ms` from its tick to its present call.
    fn at(hz: f64, work_ms: f64, previous_ms: f64) -> Paced {
        Paced {
            gap: (1e9 / hz) as u64,
            work: (work_ms * 1e6) as u64,
            previous: (previous_ms * 1e6) as u64,
        }
    }

    /// A frame held to 60 Hz after a frame that took 12 ms: an explained miss at 120 Hz, and
    /// too much work to fit 120 Hz.
    fn held() -> Paced {
        at(60.0, 12.0, 12.0)
    }

    /// A frame on time at 60 Hz with 1 ms of work, after a 1 ms frame.
    fn quick_60() -> Paced {
        at(60.0, 1.0, 1.0)
    }

    /// Feeds `frames` and returns every step with the index of the frame that caused it.
    fn feed(rule: &mut RateRule, frames: impl IntoIterator<Item = Paced>) -> Vec<(usize, Step)> {
        frames
            .into_iter()
            .enumerate()
            .filter_map(|(index, paced)| rule.frame(paced).map(|step| (index, step)))
            .collect()
    }

    /// A rule for a 120 Hz display that has stepped down to 60 Hz.
    fn at_60() -> RateRule {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        assert_eq!(
            feed(&mut rule, (0..60).map(|_| held())),
            vec![(59, Step::Rate(60.0))]
        );
        rule
    }

    #[test]
    fn frames_on_time_keep_the_display_rate() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        assert_eq!(
            feed(&mut rule, (0..1000).map(|_| at(120.0, 1.0, 1.0))),
            vec![]
        );
        assert!((rule.hz() - 120.0).abs() < f32::EPSILON);
    }

    #[test]
    fn frames_while_the_window_settles_are_not_counted() {
        let thresholds = Thresholds {
            settle_frames: 120,
            ..THRESHOLDS
        };
        let mut rule = RateRule::new(120.0, false, thresholds);
        assert_eq!(feed(&mut rule, (0..120).map(|_| held())), vec![]);
        assert_eq!(
            feed(&mut rule, (0..60).map(|_| held())),
            vec![(59, Step::Rate(60.0))]
        );
    }

    #[test]
    fn scattered_late_ticks_after_short_frames_never_step() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        let frames = (0..10_000).map(|i| {
            if i % 60 < 3 {
                at(60.0, 1.0, 1.0)
            } else {
                at(120.0, 1.0, 1.0)
            }
        });
        assert_eq!(feed(&mut rule, frames), vec![]);
    }

    #[test]
    fn a_display_held_to_60_with_little_work_does_not_step() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        assert_eq!(feed(&mut rule, (0..10_000).map(|_| quick_60())), vec![]);
    }

    #[test]
    fn a_display_held_to_60_with_long_frames_steps_down_once() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        // Once at 60 Hz the frames are on time, and 12 ms of work leaves no room for 120 Hz.
        assert_eq!(
            feed(&mut rule, (0..10_000).map(|_| held())),
            vec![(59, Step::Rate(60.0))]
        );
    }

    #[test]
    fn fewer_explained_misses_than_needed_in_a_window_do_not_step() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        // 59 explained misses in every 240 frames: no 240 frames in a row hold 60.
        let frames = (0..2400).map(|i| {
            if i % 240 < 59 {
                held()
            } else {
                at(120.0, 1.0, 1.0)
            }
        });
        assert_eq!(feed(&mut rule, frames), vec![]);
    }

    #[test]
    fn steps_up_after_a_run_with_few_late_frames() {
        let mut rule = at_60();
        // 30 of 600 frames late at 60 Hz, each after a short frame.
        let frames = (0..600).map(|i| {
            if i % 20 == 0 {
                at(25.0, 1.0, 1.0)
            } else {
                quick_60()
            }
        });
        assert_eq!(feed(&mut rule, frames), vec![(599, Step::Rate(120.0))]);
    }

    #[test]
    fn one_late_frame_too_many_restarts_the_run() {
        let mut rule = at_60();
        // The 31st late frame, at index 30, restarts the run, which then needs 600 more.
        let frames = (0..700).map(|i| {
            if i < 31 {
                at(25.0, 1.0, 1.0)
            } else {
                quick_60()
            }
        });
        assert_eq!(feed(&mut rule, frames), vec![(630, Step::Rate(120.0))]);
    }

    #[test]
    fn frames_without_room_count_against_the_run() {
        let mut rule = at_60();
        // On time at 60 Hz, but 5 ms of work is more than half of 120 Hz's 8.3 ms, so every
        // 31st frame restarts the run, and the last of these leaves it empty.
        assert_eq!(
            feed(&mut rule, (0..31 * 60).map(|_| at(60.0, 5.0, 5.0))),
            vec![]
        );
        assert_eq!(
            feed(&mut rule, (0..600).map(|_| quick_60())),
            vec![(599, Step::Rate(120.0))]
        );
    }

    #[test]
    fn a_late_frame_after_a_step_up_is_not_a_failure() {
        let mut rule = at_60();
        feed(&mut rule, (0..600).map(|_| quick_60()));
        assert!((rule.hz() - 120.0).abs() < f32::EPSILON);
        let mut frames = vec![at(60.0, 1.0, 1.0)];
        frames.extend((0..700).map(|_| at(120.0, 1.0, 1.0)));
        assert_eq!(feed(&mut rule, frames), vec![]);
        assert_eq!(
            feed(&mut rule, (0..60).map(|_| held())),
            vec![(59, Step::Rate(60.0))]
        );
        assert_eq!(
            feed(&mut rule, (0..600).map(|_| quick_60())),
            vec![(599, Step::Rate(120.0))]
        );
    }

    #[test]
    fn a_step_down_soon_after_a_step_up_doubles_the_next_run() {
        let mut rule = at_60();
        feed(&mut rule, (0..600).map(|_| quick_60()));
        assert_eq!(
            feed(&mut rule, (0..60).map(|_| held())),
            vec![(59, Step::Rate(60.0))]
        );
        assert_eq!(
            feed(&mut rule, (0..1200).map(|_| quick_60())),
            vec![(1199, Step::Rate(120.0))]
        );
    }

    #[test]
    fn the_tier_drops_before_the_rate() {
        let mut rule = RateRule::new(120.0, true, THRESHOLDS);
        let steps = feed(&mut rule, (0..200).map(|_| held()));
        assert_eq!(steps, vec![(59, Step::Baseline), (119, Step::Rate(60.0))]);
    }

    #[test]
    fn frames_too_slow_for_60_step_down_to_40_and_30() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        let steps = feed(&mut rule, (0..400).map(|_| at(25.0, 20.0, 20.0)));
        assert_eq!(
            steps,
            vec![
                (59, Step::Rate(60.0)),
                (119, Step::Rate(40.0)),
                (179, Step::Rate(30.0))
            ]
        );
        // 30 Hz is the lowest: frames still late ask for nothing more.
        assert_eq!(
            feed(&mut rule, (0..400).map(|_| at(15.0, 40.0, 40.0))),
            vec![]
        );
    }

    #[test]
    fn a_60_hz_display_steps_down_to_30_and_no_lower() {
        let mut rule = RateRule::new(60.0, false, THRESHOLDS);
        let steps = feed(&mut rule, (0..400).map(|_| at(10.0, 90.0, 90.0)));
        assert_eq!(steps, vec![(59, Step::Rate(30.0))]);
    }

    #[test]
    fn a_rate_that_fails_after_each_step_up_is_tried_less_often() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        // The window server holds every frame to 60 Hz whatever the app asks, so each frame
        // waits for a drawable, while the app's own work is small: the trap a naive rule falls
        // into, stepping up and down for ever.
        let mut ups = Vec::new();
        let mut downs = 0;
        for index in 0..60_000 {
            if let Some(step) = rule.frame(at(60.0, 1.0, 12.0)) {
                match step {
                    Step::Rate(hz) if hz > 61.0 => ups.push(index),
                    Step::Rate(_) => downs += 1,
                    Step::Baseline => unreachable!(),
                }
            }
        }
        // Each failed step up doubles the wait for the next, up to 9600 frames.
        let waits: Vec<usize> = ups.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert_eq!(waits.first().copied(), Some(1260));
        assert!(waits.windows(2).all(|pair| pair[1] >= pair[0]), "{waits:?}");
        assert!(waits.iter().all(|&wait| wait <= 9600 + 60), "{waits:?}");
        assert!(downs <= ups.len() + 1);
        assert!(ups.len() <= 10, "{}", ups.len());
    }
}
