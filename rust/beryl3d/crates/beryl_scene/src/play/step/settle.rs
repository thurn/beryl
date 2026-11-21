//! Cards that move a fraction of a millimetre within their pile take no segment.
//!
//! On the desktop a pile stands so that its top card stays where the web interface draws it
//! (`table::FittedPiles`), so a card that joins or leaves a pile moves every card left in it a
//! little. A segment for that would put each such card on a track, and a card on a track is
//! shown at the place the script leads to (such as a hand it is about to be drawn into) while it
//! still lies in its pile. So a card that no step has moved yet stays where it lies: steps that
//! move it later start from there, and the table takes its new place when the script ends.

use super::Motion;
use crate::table::Rest;
use crate::verbs::Interval;
use beryl_protocol::protocol::ZoneName;
use kit_base::pose::Pose;
use kit_frame::frame::Place;

/// How far a card may move within its pile and take no segment, in millimetres: a few cards'
/// worth of the pile's shift.
const PILE_SETTLE: f32 = 2.0;

impl Motion<'_> {
    /// Whether the card in `slot`, which the step gives a new resting place, only settles
    /// within its pile and takes no segment. If so, notes where it lay before, the first time.
    pub(super) fn settles(&mut self, slot: usize) -> bool {
        let tracked = (self.tracks.get(slot)).is_some_and(|track| !track.segments().is_empty());
        let before = self.rests.rests.get(slot).and_then(Option::as_ref);
        let after = self.fresh.rests.get(slot).and_then(Option::as_ref);
        let (false, Some(before), Some(after)) = (tracked, before, after) else {
            return false;
        };
        if !settles_in_pile(before, after) {
            return false;
        }
        if self.settled_pose(slot).is_none() {
            self.settled.push((slot, before.pose));
        }
        true
    }

    /// The interval over which the card in `slot` slides to its new resting place in a step
    /// whose own interval is `own` and which gives the cards it moves `room`. A card that an
    /// earlier step of the script gave a track, and that only settles within its pile now, slides
    /// over `own`, so that the beat does not wait for it. A mulligan vanishes the hand into the
    /// library one card at a time, and each card that joins moves those already there a fraction
    /// of a millimetre. Sliding them over each vanish's closing room held the beat open for 40%
    /// of a vanish longer than the web interface's, whose pile draws no such move, so the new
    /// hand appeared 158 ms late.
    pub(super) fn slide_room(&self, slot: usize, own: Interval, room: Interval) -> Interval {
        let before = self.rests.rests.get(slot).and_then(Option::as_ref);
        let after = self.fresh.rests.get(slot).and_then(Option::as_ref);
        let tracked = (self.tracks.get(slot)).is_some_and(|track| !track.segments().is_empty());
        let settles = before
            .zip(after)
            .is_some_and(|(b, a)| settles_in_pile(b, a));
        if tracked && settles { own } else { room }
    }

    /// Where the card in `slot` lies, if it only settled within its pile so far.
    pub(super) fn settled_pose(&self, slot: usize) -> Option<Pose> {
        (self.settled.iter())
            .find(|(settled, _)| *settled == slot)
            .map(|(_, pose)| *pose)
    }
}

/// Whether a card resting at `before` and then at `after` stays in the same library, graveyard
/// or exile pile and moves no more than `PILE_SETTLE`.
fn settles_in_pile(before: &Rest, after: &Rest) -> bool {
    let in_pile = matches!(
        before.place,
        Place::Card {
            zone: ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile,
            ..
        }
    );
    in_pile
        && before.place == after.place
        && before.pose.rotation == after.pose.rotation
        && before.pose.position.distance(after.pose.position) <= PILE_SETTLE
}
