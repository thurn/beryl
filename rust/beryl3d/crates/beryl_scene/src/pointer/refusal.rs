//! The shake of a card whose table action the manual table refused (docs/ui-conventions.md,
//! "Manual table: a click acts": "A refused action shakes what was clicked"), as the web's
//! `shakeRefusal` (`ui/src/stage/Refusal.tsx`) moves the clicked card's face: across the
//! screen through `OFFSETS` over `refusal_ms`, eased out, while the card stays where the table
//! has it.

use crate::data::space::pointer as data;
use crate::pose::Curve;

/// Nanoseconds in a millisecond.
const MILLISECOND: f32 = 1_000_000.0;

/// How far across the screen the card is at evenly spaced points of the shake, in logical
/// points: `shakeRefusal`'s keyframes of `translate`, in CSS pixels.
const OFFSETS: [f32; 6] = [0.0, -5.0, 4.0, -3.0, 2.0, 0.0];

/// One card shaking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Shake {
    pub slot: usize,
    /// When it began: the first frame after the refusal arrived, since a reply carries no time.
    pub since: Option<u64>,
}

impl Shake {
    /// How long the shake takes, in nanoseconds.
    fn length() -> f32 {
        data::REFUSAL_MS * MILLISECOND
    }

    /// How far across the screen the card is at `now`, in logical points.
    pub fn offset(&self, now: u64) -> f32 {
        let Some(since) = self.since else {
            return 0.0;
        };
        let share = Curve::EASE_OUT.ease(now.saturating_sub(since) as f32 / Self::length());
        let along = share.clamp(0.0, 1.0) * (OFFSETS.len() - 1) as f32;
        let at = (along.floor() as usize).min(OFFSETS.len() - 2);
        let part = along - at as f32;
        OFFSETS[at] + (OFFSETS[at + 1] - OFFSETS[at]) * part
    }

    /// Whether the shake is over by `now`.
    pub fn done(&self, now: u64) -> bool {
        self.since
            .is_some_and(|since| now.saturating_sub(since) as f32 >= Self::length())
    }
}
