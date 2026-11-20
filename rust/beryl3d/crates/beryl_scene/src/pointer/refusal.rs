//! The shake of what a refused table action was sent from (docs/ui-conventions.md, "Manual
//! table: a click acts": "A refused action shakes what was clicked"), as the web's
//! `shakeRefusal` (`ui/src/stage/Refusal.tsx`) moves the clicked element: across the screen
//! through `OFFSETS` over `refusal_ms`, eased out, while the element stays where the table has
//! it. A card's shake is held by the pointer (`Pointer::refuse`), and a flat-layer button's by
//! the client (`Client::take_table_reply`).

use crate::data::space::pointer as data;
use crate::pose::Curve;

/// Nanoseconds in a millisecond.
const MILLISECOND: f32 = 1_000_000.0;

/// How far across the screen the card is at evenly spaced points of the shake, in logical
/// points: `shakeRefusal`'s keyframes of `translate`, in CSS pixels.
const OFFSETS: [f32; 6] = [0.0, -5.0, 4.0, -3.0, 2.0, 0.0];

/// One element shaking.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shake {
    /// When it began: the first frame after the refusal arrived.
    pub since: Option<u64>,
}

impl Shake {
    /// A shake that begins at the first frame after it is made, since a reply carries no time.
    #[must_use]
    pub const fn new() -> Self {
        Self { since: None }
    }

    /// Starts the shake at `now` unless it has begun, and returns whether it is still under
    /// way: the client calls this at the start of each frame.
    pub fn advance(&mut self, now: u64) -> bool {
        self.since.get_or_insert(now);
        !self.done(now)
    }

    /// How long the shake takes, in nanoseconds.
    fn length() -> f32 {
        data::REFUSAL_MS * MILLISECOND
    }

    /// How far across the screen the element is at `now`, in logical points.
    #[must_use]
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
    #[must_use]
    pub fn done(&self, now: u64) -> bool {
        self.since
            .is_some_and(|since| now.saturating_sub(since) as f32 >= Self::length())
    }
}
