//! Measures submissions from successful draws on the host's monotonic clock.

use std::collections::VecDeque;

use app_client::FrameRates;

const SECOND: u64 = 1_000_000_000;
const SAMPLE: u64 = SECOND / 2;
const MINUTE: u64 = 60 * SECOND;

#[derive(Default)]
pub(super) struct Monitor {
    pub hidden: bool,
    pub suspended: bool,
    pub samples: Option<Samples>,
}

#[derive(Default)]
pub(super) struct Samples {
    since: Option<u64>,
    drawn: u32,
    last_shown: Option<u64>,
    // The largest gap is first. An older gap no longer matters once a newer gap is at
    // least as large. Popping both ends keeps updates cheap without rounding the cutoff.
    slowest: VecDeque<(u64, u64)>,
}

impl Samples {
    pub fn sample(&mut self, now: u64, target: f32, cadence: Option<f64>) -> Option<FrameRates> {
        let current = if let Some(since) = self.since {
            let elapsed = now.saturating_sub(since);
            if elapsed < SAMPLE {
                return None;
            }
            Some(rate(f64::from(self.drawn), elapsed))
        } else {
            None
        };
        self.since = Some(now);
        self.drawn = 0;
        while self
            .slowest
            .front()
            .is_some_and(|&(at, _)| now.saturating_sub(at) >= MINUTE)
        {
            self.slowest.pop_front();
        }
        Some(FrameRates {
            current,
            low: self.slowest.front().map(|&(_, gap)| rate(1.0, gap)),
            target: Some(target.round() as u16),
            cadence: cadence.map(|hz| hz.round().max(1.0) as u16),
        })
    }

    pub fn shown(&mut self, now: u64) {
        self.drawn += 1;
        if let Some(last) = self.last_shown.replace(now)
            && now > last
        {
            let gap = now - last;
            while self.slowest.back().is_some_and(|&(_, older)| older <= gap) {
                self.slowest.pop_back();
            }
            self.slowest.push_back((now, gap));
        }
    }

    /// The next frame starts its own sample, with no gap across the time spent hidden.
    pub fn pause(&mut self) {
        self.since = None;
        self.last_shown = None;
    }
}

fn rate(frames: f64, elapsed: u64) -> u16 {
    (frames * SECOND as f64 / elapsed as f64).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_draw_lowers_throughput_and_the_worst_frame_ages_out() {
        let mut monitor = Samples::default();
        let tick = SECOND / 120;
        assert_eq!(monitor.sample(0, 120.0, Some(60.0)).unwrap().current, None);
        for frame in 0..60 {
            if frame != 20 {
                monitor.shown(frame * tick);
            }
        }
        let live = monitor.sample(SAMPLE, 120.0, Some(120.0)).unwrap();
        assert_eq!(live.current, Some(118));
        assert_eq!(live.low, Some(60));
        assert_eq!(live.target, Some(120));
        assert_eq!(live.cadence, Some(120));
        assert_eq!(monitor.sample(SAMPLE + tick, 120.0, None), None);

        monitor.pause();
        let still_recent = monitor.sample(MINUTE + 20 * tick, 60.0, None).unwrap();
        assert_eq!(still_recent.current, None);
        assert_eq!(still_recent.low, Some(60));
        assert_eq!(still_recent.target, Some(60));
        monitor.shown(MINUTE + 20 * tick);
        monitor.shown(MINUTE + 22 * tick);
        let recovered = monitor.sample(MINUTE + SECOND, 60.0, None).unwrap();
        assert_eq!(recovered.low, Some(60));
    }

    #[test]
    fn a_stall_expires_and_an_intentional_pause_leaves_no_gap() {
        let mut monitor = Samples::default();
        monitor.sample(0, 60.0, None);
        monitor.shown(0);
        monitor.shown(SECOND / 4);
        assert_eq!(monitor.sample(SAMPLE, 60.0, None).unwrap().low, Some(4));

        monitor.pause();
        assert_eq!(
            monitor.sample(MINUTE + SECOND / 4, 60.0, None).unwrap().low,
            None
        );
        monitor.shown(MINUTE + SECOND / 4);
        monitor.shown(MINUTE + SECOND / 4 + SECOND / 60);
        assert_eq!(
            monitor.sample(MINUTE + SECOND, 60.0, None).unwrap().low,
            Some(60)
        );
    }
}
