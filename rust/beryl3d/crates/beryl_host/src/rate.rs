//! The frame rate a host asks the display for, and when it steps down or back up (decision C18
//! in docs/beryl-3d.md: the highest rate the display and device can hold, but smooth motion
//! comes first). Every host feeds this rule the time between its animating frames and carries
//! out the steps it returns: the macOS display link changes its preferred frame rate range, and
//! the phone hosts will change `CADisplayLink`'s and `Surface.setFrameRate`'s.
//!
//! - **Settling.** The first `settle_frames` animating frames after the host opens are not
//!   counted: the first second of a new window was often late while the system set it up.
//! - **A frame misses the rate** when more than `late_intervals` of the chosen rate's
//!   intervals passed since the frame before it, while the ticks ran.
//! - **Stepping down.** When `misses` of the last `window` animating frames missed, the rule
//!   first drops the Enhanced tier to Baseline, if the tier is chosen automatically and is
//!   Enhanced. If frames still miss, it asks for the next rate that divides the display's
//!   (120 to 60 to 40 to 30 Hz), down to `lowest_hz`. An even lower rate beats an uneven
//!   higher one.
//! - **Stepping back up.** After `up_frames` animating frames in a row that came on time and
//!   whose own work (the client's frame and encoding it) took no more than `room` of the
//!   higher rate's interval, it asks for the next rate up.
//! - **Not going back and forth.** When frames miss again within `up_frames` frames of a step
//!   up, the step up failed, and the run the next one needs doubles, up to `up_frames_most`.
//!   A device whose window server cannot hold the higher rate therefore tries it less and less
//!   often instead of alternating.
//!
//! The thresholds are `frame_rate` in `data/beryl3d/space.json`.

use beryl_scene::data::space::frame_rate;

/// What the rule needs to know of one animating frame, in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paced {
    /// The time since the frame before it, while the ticks ran.
    pub gap: u64,
    /// The app's own work for the frame: the client's frame and encoding its GPU commands,
    /// without the wait for a drawable.
    pub work: u64,
}

/// A change the host must make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// Draw in the Baseline tier for the rest of the session (decision C8).
    Baseline,
    /// Ask the display for this rate, in frames a second.
    Rate(f32),
}

/// The thresholds of the rule (`frame_rate` in `data/beryl3d/space.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub settle_frames: usize,
    pub late_intervals: f32,
    pub window: usize,
    pub misses: usize,
    pub room: f32,
    pub up_frames: usize,
    pub up_frames_most: usize,
    pub lowest_hz: usize,
}

impl Thresholds {
    /// The thresholds in `data/beryl3d/space.json`.
    pub const DATA: Self = Self {
        settle_frames: frame_rate::SETTLE_FRAMES,
        late_intervals: frame_rate::LATE_INTERVALS,
        window: frame_rate::WINDOW,
        misses: frame_rate::MISSES,
        room: frame_rate::ROOM,
        up_frames: frame_rate::UP_FRAMES,
        up_frames_most: frame_rate::UP_FRAMES_MOST,
        lowest_hz: frame_rate::LOWEST_HZ,
    };
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
    /// Whether each of the last `window` frames missed, as a ring, and how many did.
    missed: Vec<bool>,
    next: usize,
    missed_count: usize,
    /// Frames in a row with room to spare at the next rate up.
    roomy: usize,
    /// The run of roomy frames the next step up needs.
    up_frames: usize,
    /// Frames since the last step up, while it may still fail.
    since_up: Option<usize>,
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
            roomy: 0,
            up_frames: thresholds.up_frames,
            since_up: None,
        }
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

    /// Counts an animating frame, and returns the step the host must take, if any.
    pub fn frame(&mut self, paced: Paced) -> Option<Step> {
        if self.settling > 0 {
            self.settling -= 1;
            return None;
        }
        let missed = self.misses(paced.gap);
        if let Some(since) = &mut self.since_up {
            *since += 1;
            if missed {
                // Frames missed soon after a step up: the next one waits twice as long.
                self.up_frames = (self.up_frames * 2).min(self.thresholds.up_frames_most);
                self.since_up = None;
            } else if *since >= self.thresholds.up_frames {
                // The step up held: the next one needs only the ordinary run.
                self.up_frames = self.thresholds.up_frames;
                self.since_up = None;
            }
        }
        if self.missed[self.next] {
            self.missed_count -= 1;
        }
        self.missed[self.next] = missed;
        self.next = (self.next + 1) % self.missed.len();
        if missed {
            self.missed_count += 1;
        }
        if self.missed_count >= self.thresholds.misses
            && let Some(step) = self.step_down()
        {
            return Some(step);
        }
        self.roomy = if !missed && self.has_room(paced.work) {
            self.roomy + 1
        } else {
            0
        };
        if self.divisor > 1 && self.roomy >= self.up_frames {
            self.divisor -= 1;
            self.restart();
            self.since_up = Some(0);
            return Some(Step::Rate(self.hz()));
        }
        None
    }

    /// The tier, or else the rate, one step lower; `None` at the lowest rate.
    fn step_down(&mut self) -> Option<Step> {
        if self.tier_can_drop {
            self.tier_can_drop = false;
            self.restart();
            return Some(Step::Baseline);
        }
        let lower = self.display_hz / (self.divisor + 1) as f32;
        if lower < self.thresholds.lowest_hz as f32 - 0.5 {
            return None;
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
        self.roomy = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLDS: Thresholds = Thresholds {
        settle_frames: 0,
        late_intervals: 1.5,
        window: 60,
        misses: 3,
        room: 0.5,
        up_frames: 100,
        up_frames_most: 800,
        lowest_hz: 30,
    };

    /// A frame at `hz` with `work` milliseconds of the app's own work.
    fn at(hz: f64, work_ms: f64) -> Paced {
        Paced {
            gap: (1e9 / hz) as u64,
            work: (work_ms * 1e6) as u64,
        }
    }

    /// Feeds `frames` and returns every step with the index of the frame that caused it.
    fn feed(rule: &mut RateRule, frames: impl IntoIterator<Item = Paced>) -> Vec<(usize, Step)> {
        frames
            .into_iter()
            .enumerate()
            .filter_map(|(index, paced)| rule.frame(paced).map(|step| (index, step)))
            .collect()
    }

    #[test]
    fn frames_on_time_keep_the_display_rate() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        assert_eq!(feed(&mut rule, (0..1000).map(|_| at(120.0, 1.0))), vec![]);
        assert!((rule.hz() - 120.0).abs() < f32::EPSILON);
    }

    #[test]
    fn frames_while_the_window_settles_are_not_counted() {
        let thresholds = Thresholds {
            settle_frames: 120,
            ..THRESHOLDS
        };
        let mut rule = RateRule::new(120.0, false, thresholds);
        assert_eq!(feed(&mut rule, (0..120).map(|_| at(30.0, 1.0))), vec![]);
        assert_eq!(
            feed(&mut rule, (0..3).map(|_| at(30.0, 1.0))),
            vec![(2, Step::Rate(60.0))]
        );
    }

    #[test]
    fn two_late_frames_in_a_window_are_not_enough() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        let frames = (0..120).map(|i| {
            if i % 59 == 0 {
                at(60.0, 1.0)
            } else {
                at(120.0, 1.0)
            }
        });
        assert_eq!(feed(&mut rule, frames), vec![]);
    }

    #[test]
    fn the_tier_drops_before_the_rate() {
        let mut rule = RateRule::new(120.0, true, THRESHOLDS);
        let steps = feed(&mut rule, (0..10).map(|_| at(60.0, 1.0)));
        assert_eq!(steps, vec![(2, Step::Baseline), (5, Step::Rate(60.0))]);
    }

    #[test]
    fn a_held_display_steps_down_to_the_next_divisor_and_stays() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        // Shown at 60 Hz throughout, with too little room for 120 Hz: once at 60 Hz the frames
        // are on time, and the rule stays there.
        let steps = feed(&mut rule, (0..2000).map(|_| at(60.0, 6.0)));
        assert_eq!(steps, vec![(2, Step::Rate(60.0))]);
    }

    #[test]
    fn frames_too_slow_for_60_step_down_to_40() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        let steps = feed(&mut rule, (0..20).map(|_| at(25.0, 20.0)));
        assert_eq!(
            steps,
            vec![
                (2, Step::Rate(60.0)),
                (5, Step::Rate(40.0)),
                (8, Step::Rate(30.0))
            ]
        );
        // 30 Hz is the lowest: frames still late ask for nothing more.
        assert_eq!(feed(&mut rule, (0..100).map(|_| at(15.0, 40.0))), vec![]);
    }

    #[test]
    fn a_60_hz_display_steps_down_to_30_and_no_lower() {
        let mut rule = RateRule::new(60.0, false, THRESHOLDS);
        let steps = feed(&mut rule, (0..20).map(|_| at(10.0, 90.0)));
        assert_eq!(steps, vec![(2, Step::Rate(30.0))]);
    }

    #[test]
    fn steps_back_up_after_a_sustained_run_with_room() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        feed(&mut rule, (0..3).map(|_| at(60.0, 1.0)));
        assert!((rule.hz() - 60.0).abs() < f32::EPSILON);
        // On time at 60 Hz, but 5 ms of work is more than half of 120 Hz's 8.3 ms.
        assert_eq!(feed(&mut rule, (0..500).map(|_| at(60.0, 5.0))), vec![]);
        // With 1 ms of work, it steps up after `up_frames`.
        let steps = feed(&mut rule, (0..100).map(|_| at(60.0, 1.0)));
        assert_eq!(steps, vec![(99, Step::Rate(120.0))]);
    }

    #[test]
    fn a_rate_that_fails_after_each_step_up_is_tried_less_often() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        // The window server holds every frame to 60 Hz whatever the app asks, and the app's
        // own work is small: the trap a naive rule falls into, stepping up and down for ever.
        let mut ups = Vec::new();
        let mut downs = 0;
        for index in 0..20_000 {
            if let Some(step) = rule.frame(at(60.0, 1.0)) {
                match step {
                    Step::Rate(hz) if hz > 61.0 => ups.push(index),
                    Step::Rate(_) => downs += 1,
                    Step::Baseline => unreachable!(),
                }
            }
        }
        // Each failed step up doubles the wait for the next, up to 800 frames.
        let waits: Vec<usize> = ups.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(waits.windows(2).all(|pair| pair[1] >= pair[0]), "{waits:?}");
        assert!(waits.iter().all(|&wait| wait <= 800 + 3), "{waits:?}");
        assert_eq!(waits.first().copied(), Some(203));
        assert!(downs <= ups.len() + 1);
        // At 120 Hz for 3 frames out of every 803 at most, once the wait is longest.
        assert!(ups.len() <= 30, "{}", ups.len());
    }

    #[test]
    fn a_step_up_that_holds_restores_the_ordinary_wait() {
        let mut rule = RateRule::new(120.0, false, THRESHOLDS);
        feed(&mut rule, (0..3).map(|_| at(60.0, 1.0)));
        feed(&mut rule, (0..100).map(|_| at(60.0, 1.0)));
        assert!((rule.hz() - 120.0).abs() < f32::EPSILON);
        // Holding 120 Hz for longer than `up_frames`, then losing it: the step down is an
        // ordinary one, and the next step up waits the ordinary run.
        feed(&mut rule, (0..200).map(|_| at(120.0, 1.0)));
        assert_eq!(
            feed(&mut rule, (0..3).map(|_| at(60.0, 1.0))),
            vec![(2, Step::Rate(60.0))]
        );
        let steps = feed(&mut rule, (0..100).map(|_| at(60.0, 1.0)));
        assert_eq!(steps, vec![(99, Step::Rate(120.0))]);
    }
}
