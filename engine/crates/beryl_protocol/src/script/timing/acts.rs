//! Questions about one act, or about an act and its neighbours in a beat, that the timing
//! rules ask.

use std::ptr;

use crate::protocol::{Side, Why, ZoneName};
use crate::script::{Act, Subject};

/// Whether `act` is a card drawn, or a card flown as one is: the flight from a library to its
/// player's hand that the game calls a draw, or one no one at the table can read that an
/// effect puts there. A port of `draws` in `ui/src/stage/tracks.ts`.
pub(super) fn draws(act: &Act) -> bool {
    matches!(act, Act::Fly { from: ZoneName::Library, why, object, .. }
        if object.zone == ZoneName::Hand && (*why == Why::Draw || object.name.is_none()))
}

/// Whether `act` is your draw of a card you see (held up to be read; UI Conventions, `fly`).
pub(super) fn draws_to_read(act: &Act) -> bool {
    draws(act)
        && matches!(act, Act::Fly { object, .. }
            if object.zone_side.unwrap_or(object.owner) == Side::You && object.name.is_some())
}

/// Whether `a` and `b` are the two blows of one fight: each creature strikes the other.
pub(super) fn meets(a: &Act, b: Option<&Act>) -> bool {
    matches!(
        (a, b),
        (
            Act::Strike { source: a_source, to: Subject::Object { slot: a_to }, .. },
            Some(Act::Strike { source: b_source, to: Subject::Object { slot: b_to }, .. }),
        ) if a_to == b_source && b_to == a_source
    )
}

/// Whether `act` is a card's vanish or appearance that the step after it in `beside` follows
/// with a card's vanish or appearance of the same verb that goes from, or to, another zone.
pub(super) fn next_zone(act: &Act, beside: &[&Act]) -> bool {
    let Some(at) = beside.iter().position(|other| ptr::eq(*other, act)) else {
        return false;
    };
    match (act, beside.get(at + 1)) {
        (
            Act::Vanish {
                subject: Subject::Object { .. },
                from,
                ..
            },
            Some(Act::Vanish {
                subject: Subject::Object { .. },
                from: next,
                ..
            }),
        ) => next != from,
        (
            Act::Appear { from, object, .. },
            Some(Act::Appear {
                from: next_from,
                object: next,
                ..
            }),
        ) => (next_from, next.zone) != (from, object.zone),
        _ => false,
    }
}
