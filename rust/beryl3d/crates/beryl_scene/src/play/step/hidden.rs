//! Cards a step moves to a place a row has scrolled out of view, which are not drawn there.

use super::super::record::Cause;
use super::Motion;
use crate::pose::{Pose, Segment};
use crate::table::Rest;
use crate::verbs;

/// Whether a card that rested at `before` and rests at `after` goes from a place in view to one
/// a row has scrolled out of view (`Motion::hide_on_arrival`).
pub(super) fn hides(before: Option<&Rest>, after: Option<&Rest>) -> bool {
    before.is_some_and(|rest| rest.visible) && after.is_some_and(|rest| !rest.visible)
}

/// Whether a card that a step changes in place, as one that phases out does, rests out of view
/// afterwards (`Motion::hide_on_arrival`), wherever it rested before. The change gives the card
/// a segment, and a card on a track is drawn unless it rests out of view both before and after
/// the whole script. So a card out of view that a later step of the script moved into view was
/// drawn out of view from the change until that move: on the phone layout, a tapped land at the
/// lands row's hidden end stood drawn over an artifact in the others row from when a spell
/// phased it out until the lands untapped and flew back into view.
pub(super) fn hides_in_place(before: Option<&Rest>, after: Option<&Rest>) -> bool {
    before.is_some() && after.is_some_and(|rest| !rest.visible)
}

impl Motion<'_> {
    /// Notes that the `number`th step moved the object in `slot` to a place a row has scrolled
    /// out of view (`rows::beyond_right`). The scene leaves such a card undrawn only where it
    /// rests before or after the whole script, so a card that a step moved to the hidden end of
    /// a crowded row and a later step of the same script moved again stood drawn there in
    /// between, over the card in view beside it. On the phone layout, an evoked creature that
    /// entered a full row covered the permanent beside it until it was sacrificed, and a land
    /// pushed off the lands row by lands returning one by one covered a permanent of the next
    /// row until they gathered into piles. If a later step moves it (`push`), it is hidden from
    /// when it arrived until then.
    pub(super) fn hide_on_arrival(&mut self, slot: usize, number: usize) {
        if let Some(end) = (self.tracks.get(slot))
            .and_then(|track| track.segments().last())
            .map(Segment::end)
        {
            self.hidden.push((slot, end, number));
        }
    }

    /// Hides the object in `slot` from when it arrived at a place out of view
    /// (`hide_on_arrival`) until `start`, when a later step moves it again.
    pub(super) fn reveal(&mut self, slot: usize, start: u64) {
        let Some(index) = self.hidden.iter().position(|&(hidden, ..)| hidden == slot) else {
            return;
        };
        let (_, arrived, number) = self.hidden.swap_remove(index);
        if arrived < start {
            let at = self.pose(slot, arrived);
            let gone = Pose { opacity: 0.0, ..at };
            self.push(
                slot,
                verbs::instant(at, gone, arrived),
                Cause::Change(number),
            );
        }
    }
}
