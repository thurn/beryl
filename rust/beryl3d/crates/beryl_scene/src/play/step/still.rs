//! What a step that moves nothing, a `set`, does to the objects it places again: each is at its
//! new place as the step begins, with no slide.

use super::super::record::Cause;
use super::Motion;
use crate::pose::{Pose, Segment};
use crate::verbs::{self, Interval};

impl Motion<'_> {
    /// The instant change at the start of the `number`th step's `own` time that carries the
    /// object in `slot` from where it `rested` to where it rests `now`, for a step that moves
    /// nothing, or `None` where it stays.
    pub(super) fn change(
        &mut self,
        slot: usize,
        (rested, now): (Option<Pose>, Option<Pose>),
        number: usize,
        own: Interval,
    ) -> Option<(Segment, Cause)> {
        let from = self.pose(slot, own.from);
        let to = match (rested, now) {
            (_, Some(now)) => now,
            (Some(_), None) => Pose {
                opacity: 0.0,
                ..from
            },
            (None, None) => return None,
        };
        let from = if rested.is_none() {
            Pose { opacity: 0.0, ..to }
        } else {
            from
        };
        (from != to).then(|| (verbs::instant(from, to, own.from), Cause::Change(number)))
    }
}
