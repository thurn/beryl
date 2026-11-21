//! What happens to a card already on its way when a later step of its beat moves where it
//! rests: a flight lands at the new place (`retarget`), and a card fading in where it rests in
//! a hand goes to its new place within a frame, still fading (`regroup_fade`).

use super::super::record::SegmentRecord;
use crate::verbs::{self, Interval};
use kit_base::pose::{Fade, Pose, Segment, Shape};
use kit_space::motion::Track;

/// How long a card fading in where it rests takes to go to its new place, in nanoseconds: one
/// frame at 60 Hz. The web stage moves it between one frame and the next; a slide that short
/// looks the same and keeps the card on a path, which the "no jumps" check asks of every card
/// that can be seen. The slide ends when room is made, as the web stage's card stands at its
/// new place from that time on.
const MOVE: u64 = 16_666_667;

/// Sends every segment of the object in `slot` that lands at `was` and has not ended by
/// `from` to `to` instead, and says whether there was one. When several cards are drawn in
/// one beat, each later draw makes room in the hand while the cards before it are still in
/// flight, or have not left yet. A slide from `from` would be cut off by a flight that
/// begins after it, or would cut short one under way, and the card would end the script
/// at the place it had before the room was made and jump as the script ends; so the
/// flight lands at the new place.
pub(super) fn retarget(
    track: Option<&mut Track>,
    records: &mut [SegmentRecord],
    slot: usize,
    from: u64,
    was: &Pose,
    to: &Pose,
) -> bool {
    let Some(track) = track else {
        return false;
    };
    let mut found = false;
    for segment in track.segments_mut() {
        if segment.end() > from && &segment.to == was {
            segment.to = *to;
            found = true;
        }
    }
    // The same segments in the records, which the interface checks read.
    for record in records {
        let segment = &mut record.segment;
        if record.slot == slot && segment.end() > from && &segment.to == was {
            segment.to = *to;
        }
    }
    found
}

/// What `regroup_fade` did to a card fading in where it rests.
#[expect(
    clippy::large_enum_variant,
    reason = "it lives only on the stack between `regroup_fade` and its caller"
)]
pub(super) enum Regrouped {
    /// The card has no fade under way or to come that lands where it rested.
    No,
    /// The card's fade had not begun, and now plays at its new place.
    Moved,
    /// The card's fade was under way: it goes to its new place along the first segment and
    /// finishes fading there along the second.
    Split(Segment, Segment),
}

/// Moves a card that fades in where it rests, as `appear` has it, to its new place `to`, when a
/// later step of its beat makes room in its hand from `room.from`: when the opening hand is
/// dealt, each card that arrives makes room among the cards dealt before it, which are still
/// fading in. On the web stage such a card's own track only changes its opacity, and a later
/// step that moves where it rests does not take it over (`Track.own` in
/// `ui/src/stage/tracks.ts`), so the card stands wherever it rests and goes to its new place at
/// once. Redirecting the fade itself (`retarget`) made the card glide to its last place over
/// the rest of the fade instead, so each card of a dealt hand reached its place about 150 ms
/// after the web's did.
///
/// A fade that has not begun by `room.from` plays at the new place. From one under way, whole
/// or already cut short by an earlier regroup, the card slides to the new place over the `MOVE`
/// before `room.from` (from when the fade began, if that is later) and fades on there as the
/// fade was. On the web stage the card stands at its new place from `room.from`, so a slide
/// that began then was a frame behind it: `just b3d match` reads both clients at step starts,
/// and there every card the step made room for stood half a slot from the web's. Both segments
/// follow the fade's opacity (`Shape::Regrouping`) and take the place of what the card would
/// have done after the slide begins; the beat is laid out before it plays, so the slide is
/// known before it starts. The records, which the interface checks read, change with the
/// track.
pub(super) fn regroup_fade(
    track: Option<&mut Track>,
    records: &mut [SegmentRecord],
    slot: usize,
    room: Interval,
    was: &Pose,
    to: &Pose,
) -> Regrouped {
    let Some(track) = track else {
        return Regrouped::No;
    };
    let fading = |segment: &Segment| {
        segment.end() > room.from && &segment.to == was && fade_of(segment).is_some()
    };
    let Some(index) = track.segments().iter().rposition(fading) else {
        return Regrouped::No;
    };
    let segment = track.segments()[index];
    let Some((began, fade)) = fade_of(&segment) else {
        return Regrouped::No;
    };
    if began >= room.from {
        let moved = |segment: &mut Segment| {
            segment.from = Pose {
                opacity: segment.from.opacity,
                ..*to
            };
            segment.to = *to;
        };
        moved(&mut track.segments_mut()[index]);
        for record in records {
            if record.slot == slot
                && record.segment.start == segment.start
                && fading(&record.segment)
            {
                moved(&mut record.segment);
            }
        }
        return Regrouped::Moved;
    }
    let regrouping = |mut part: Segment| {
        part.shape = Shape::Regrouping(Fade {
            begun: part.start - began,
            ..fade
        });
        part
    };
    let moved = room.from;
    let leaves = room.from.saturating_sub(MOVE).max(began);
    let there = Pose {
        opacity: Fade { begun: 0, ..fade }.opacity(moved - began),
        ..*to
    };
    let slide = verbs::slide(
        track.pose_at(leaves),
        there,
        Interval {
            from: leaves,
            to: moved,
        },
    );
    let rest = Segment::between(
        there,
        *to,
        Interval {
            from: moved,
            to: (began + fade.duration).max(moved),
        },
        fade.curve,
    );
    Regrouped::Split(regrouping(slide), regrouping(rest))
}

/// The fade `segment` plays and when it began, in nanoseconds, if it only changes its object's
/// opacity, along a straight line: a card fading in where it rests, or what is left of that
/// fade after an earlier regroup (`Shape::Regrouping`).
fn fade_of(segment: &Segment) -> Option<(u64, Fade)> {
    let in_place = segment.arc == 0.0
        && (segment.from.opacity - segment.to.opacity).abs() > f32::EPSILON
        && Pose {
            opacity: segment.to.opacity,
            ..segment.from
        } == segment.to;
    match segment.shape {
        _ if !in_place => None,
        Shape::Line => Some((
            segment.start,
            Fade {
                begun: 0,
                duration: segment.duration,
                curve: segment.curve,
                from: segment.from.opacity,
                to: segment.to.opacity,
            },
        )),
        Shape::Regrouping(fade) => Some((segment.start - fade.begun, fade)),
        _ => None,
    }
}
