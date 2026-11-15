//! When the cards a step makes room among go to their new places.

use beryl_protocol::script::{Room, StepTiming};

use super::super::compile::nanoseconds;
use crate::beat::Laid;
use crate::verbs::Interval;

/// When what the step makes room among goes to its new place, given its `timing`, when it was
/// `laid` out and its `own` interval: what makes room for it does so before it begins, where its
/// act went ahead of it; after it, where it says how long the room it left takes to close; and
/// otherwise as its `room` says, over the step or from a share of it. And, where its `room` says
/// so, when the cards that stay in a hand a card joins or leaves go to their new places: in the
/// time it gives, from when its act is applied, whatever else the step says, as the web stage's
/// do (`carried` in `ui/src/stage/tracks.ts`).
pub(super) fn room_of(
    timing: &StepTiming,
    laid: &Laid,
    own: Interval,
) -> (Interval, Option<Interval>) {
    let room = match (laid.opens, timing.closes, timing.room) {
        (Some(opens), _, _) => Interval {
            from: nanoseconds(opens.at),
            to: nanoseconds(opens.by),
        },
        (None, Some(closes), _) => Interval {
            from: own.to,
            to: nanoseconds(laid.to + closes),
        },
        (None, None, Room::From { share }) => Interval {
            from: nanoseconds(laid.from + share * (laid.to - laid.from)),
            to: own.to,
        },
        (None, None, Room::Step | Room::For { .. }) => own,
    };
    let regroup = match timing.room {
        Room::For { ms } => {
            let applied = laid.opens.map_or(laid.from, |opens| opens.at);
            Some(Interval {
                from: nanoseconds(applied),
                to: nanoseconds(applied + ms),
            })
        }
        Room::Step | Room::From { .. } => None,
    };
    (room, regroup)
}
