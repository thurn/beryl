//! Cards that move a fraction of a millimetre within their pile take no segment.
//!
//! On the desktop a pile stands so that its top card stays where the web interface draws it
//! (`table::FittedPiles`), so a card that joins or leaves a pile moves every card left in it a
//! little. A segment for that would put each such card on a track, and a card on a track is
//! shown at the place the script leads to (such as a hand it is about to be drawn into) while it
//! still lies in its pile. So a card that no step has moved yet stays where it lies: steps that
//! move it later start from there, and the table takes its new place when the script ends.

use super::Motion;
use crate::frame::Place;
use crate::pose::Pose;
use crate::table::Rest;
use beryl_protocol::protocol::ZoneName;

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
