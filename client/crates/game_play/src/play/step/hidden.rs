//! Cards a script leaves outside the window of a container that scrolls (a battlefield row, the
//! stack, a hand), which are not drawn there.
//!
//! The arrangement gives a card outside a scrolled window the place of the window's nearer end,
//! marked out of view (`Rest::visible`), and the scene does not draw a card resting there. While
//! a script plays, a card on a track is drawn at its pose instead, so playback applies one rule
//! to every segment it adds (`Motion::push`): from the end of the step that leaves a card
//! resting outside its container's window, until a step moves it into view or the script ends,
//! the card is not drawn; and a card that rests outside the window before a step and after it,
//! in the same container, is not drawn while that step moves it.
//!
//! Before this rule, playback hid such a card only in the cases found one by one, and missed
//! others. On the phone layout, a Storm copy pushed out of the stack's five places stayed drawn
//! under its neighbour while each later copy changed every item's depth, and tapped lands at the
//! lands row's hidden end were drawn over the next row while the lands before them were exiled.

use super::super::record::{Cause, SegmentRecord};
use super::Motion;
use crate::verbs::{self, Interval};
use beryl_protocol::protocol::ZoneName;
use game_table::table::{Place, Placement, Rest};
use kit_base::pose::{Pose, Segment};
use kit_space::motion::Track;

/// Whether `rest` is a place a container that scrolls has scrolled out of view: a row (of the
/// battlefield, or of the cards exiled until a permanent leaves), the stack or a hand. A card
/// beneath the top two of a library, graveyard or exile pile is also out of view, but no pile
/// scrolls, and the scene leaves such a card undrawn while it stays in its pile.
pub(super) fn scrolled_out(rest: Option<&Rest>) -> bool {
    rest.is_some_and(|rest| {
        !rest.visible
            && matches!(
                rest.place,
                Place::Card { row: Some(_), .. }
                    | Place::Card {
                        zone: ZoneName::Stack | ZoneName::Hand,
                        ..
                    }
            )
    })
}

/// Whether a card resting at `before` and then at `after` goes into or out of the window of a
/// container that scrolls without moving: the window's nearer end is both the last place in
/// view and where the cards beyond it rest, so a card there that one more item pushes out of
/// view keeps its pose (`Motion::show_or_hide`).
pub(super) fn shown_or_hidden(before: Option<&Rest>, after: Option<&Rest>) -> bool {
    let (Some(before), Some(after)) = (before, after) else {
        return false;
    };
    before.pose == after.pose && scrolled_out(Some(before)) != scrolled_out(Some(after))
}

impl Motion<'_> {
    /// Adds `segment` to the track of the object in `slot`, made for `cause`, as it is drawn
    /// where a container that scrolls holds the card out of view.
    pub(super) fn push(&mut self, slot: usize, segment: Segment, cause: Cause) {
        let segment = Segment {
            step: cause.step_time(),
            ..segment
        };
        self.reveal(slot, segment.start);
        let segment = self.as_drawn(slot, segment);
        self.append(slot, segment, cause);
        self.hide_on_arrival(slot);
    }

    /// Hides the object in `slot` as the `number`th step, played over `own`, ends, if the step
    /// leaves it outside its container's window where it stands in view (`shown_or_hidden`),
    /// or shows it then if the step brings it into view there; says whether it did. Such a card
    /// takes no slide, which would hold its beat open for a move that goes nowhere. On the
    /// phone layout, as each Storm copy went on the stack, the item it pushed out of the
    /// stack's five places stayed drawn under the item that took its place.
    pub(super) fn show_or_hide(&mut self, slot: usize, number: usize, own: Interval) -> bool {
        let before = self.rests.rests.get(slot).and_then(Option::as_ref);
        let after = self.fresh.rests.get(slot).and_then(Option::as_ref);
        let Some(after) = after.filter(|&after| shown_or_hidden(before, Some(after))) else {
            return false;
        };
        let opacity = if after.visible {
            after.pose.opacity
        } else {
            0.0
        };
        let to = Pose {
            opacity,
            ..after.pose
        };
        let from = self.pose(slot, own.to);
        self.push(
            slot,
            verbs::instant(from, to, own.to),
            Cause::Change(number),
        );
        true
    }

    /// Where the object in `slot` rests before the step being played and after it, while a
    /// step is played; `None` outside one.
    fn step_rests(&self, slot: usize) -> Option<(Option<&Rest>, Option<&Rest>)> {
        let (dirty, _) = self.playing.as_ref()?;
        let before = self.rests.rests.get(slot).and_then(Option::as_ref);
        Some((before, self.now(slot, dirty)))
    }

    /// `segment`, which the step being played gives the object in `slot`, as it is drawn. If the
    /// card rests outside the same container's window before the step and after it, the step
    /// moves it out of view, and it is drawn nowhere along `segment`. When a step moves a hidden
    /// card back into view or out of the container, it leaves at its resting opacity rather than
    /// fading in from zero, keeping cards opaque during slides.
    pub(super) fn as_drawn(&self, slot: usize, mut segment: Segment) -> Segment {
        let Some((before, after)) = self.step_rests(slot) else {
            return segment;
        };
        if !scrolled_out(before) {
            return segment;
        }
        let stays = scrolled_out(after) && before.map(|rest| rest.place) == after.map(|r| r.place);
        if stays {
            segment.from.opacity = 0.0;
            segment.to.opacity = 0.0;
        } else if let Some(rest) = before
            && segment.from.opacity <= 0.0
        {
            segment.from.opacity = rest.pose.opacity;
        }
        segment
    }

    /// Notes, after the step being played gave the object in `slot` a segment, whether it then
    /// rests outside its container's window having been in view: if a later step moves it,
    /// `reveal` hides it from when it arrived until then. One that rests out of view before the
    /// step too is hidden along the step's segments already (`as_drawn`). At the end of the
    /// script the scene leaves the card undrawn where it rests.
    pub(super) fn hide_on_arrival(&mut self, slot: usize) {
        let Some((before, after)) = self.step_rests(slot) else {
            return;
        };
        let stays = scrolled_out(before) && before.map(|rest| rest.place) == after.map(|r| r.place);
        if !scrolled_out(after) || stays {
            return;
        }
        let Some((_, number)) = self.playing else {
            return;
        };
        let Some(end) = (self.tracks.get(slot))
            .and_then(|track| track.segments().last())
            .map(Segment::end)
        else {
            return;
        };
        if let Some(entry) = self.hidden.iter_mut().find(|(hidden, ..)| *hidden == slot) {
            *entry = (slot, end, number);
        } else {
            self.hidden.push((slot, end, number));
        }
    }

    /// Hides the object in `slot` from when an earlier step left it outside its container's
    /// window (`hide_on_arrival`) until `start`, when a later step moves it again. A later
    /// segment of the same step, such as the second part of a flight that pauses, is part of
    /// the same move and hides nothing.
    pub(super) fn reveal(&mut self, slot: usize, start: u64) {
        let Some(index) = self.hidden.iter().position(|&(hidden, ..)| hidden == slot) else {
            return;
        };
        let (_, arrived, number) = self.hidden[index];
        if self.playing.is_some_and(|(_, playing)| playing == number) {
            return;
        }
        self.hidden.swap_remove(index);
        if arrived < start {
            let at = self.pose(slot, arrived);
            let gone = Pose { opacity: 0.0, ..at };
            self.append(
                slot,
                verbs::instant(at, gone, arrived),
                Cause::Change(number),
            );
        }
    }
}

/// Ends the track of each card this rule left unseen at the end of a script, and that the view
/// the script leads to puts out of view (`placement`), at the pose it rests in there. The
/// scene does not draw a card resting out of view once its track has settled, and the checks
/// ask that every track end where its card rests (`rest_equals_placement`).
pub(crate) fn restore_hidden(
    tracks: &mut [Track],
    records: &mut Vec<SegmentRecord>,
    placement: &Placement,
) {
    for (slot, track) in tracks.iter_mut().enumerate() {
        let Some(&last) = track.segments().last() else {
            continue;
        };
        let Some(rest) = placement.rests.get(slot).and_then(Option::as_ref) else {
            continue;
        };
        if last.to.opacity > 0.0 || rest.visible {
            continue;
        }
        let restore = verbs::instant(last.to, rest.pose, last.end());
        track.append(restore);
        if let Some(cause) = (records.iter().rev())
            .find(|record| record.slot == slot)
            .map(|record| record.cause)
        {
            records.push(SegmentRecord {
                slot,
                cause,
                segment: restore,
            });
        }
    }
}
