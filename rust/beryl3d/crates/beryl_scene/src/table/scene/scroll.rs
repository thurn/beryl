//! A battlefield row, the viewer's hand or the stack, scrolling (docs/beryl-3d.md, "Objects and
//! placement"). A line of cards with more than fit at the least strip shows as many groups as
//! fit from its scroll offset, which the table keeps for each row of each side, the hand and
//! the stack. When the pointer changes an offset, every object is placed again; each card that
//! stays in view slides to its new place over `slide.regroup_ms`, and each card that comes into
//! view fades in where it rests. A card that leaves the view is no longer drawn.

use beryl_protocol::protocol::{Side, ZoneName};

use super::{SceneObject, Table};
use crate::table::place::{row_index, side_index};
use crate::table::{RowWindow, Rows, Scroller};
use kit_base::data::space::slide::REGROUP_MS;
use kit_base::pose::between;
use kit_base::pose::{Curve, Pose};
use kit_frame::frame::Place;

/// Nanoseconds in a millisecond.
const MILLISECOND: f32 = 1_000_000.0;

/// The cards of the line scrolled last, and where each was drawn when it scrolled.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Slide {
    /// The line scrolled, or `None` before any has scrolled.
    place: Option<Scroller>,
    /// By slot, for each card of the row: its pose when the row scrolled, and whether it was
    /// drawn then.
    from: Vec<Option<(Pose, bool)>>,
    /// When the slide began: `None` until the first frame after the scroll, which is when it
    /// begins.
    since: Option<u64>,
    /// The `now` of the latest frame.
    now: u64,
}

impl Slide {
    /// How far through the slide is at `now`, eased, or `None` when none is under way.
    fn share(&self, now: u64) -> Option<f32> {
        self.place?;
        let elapsed = now.saturating_sub(self.since.unwrap_or(now)) as f32;
        let length = REGROUP_MS * MILLISECOND;
        (elapsed < length).then(|| Curve::EASE_OUT.ease(elapsed / length))
    }
}

impl Table {
    /// Which groups of each line of cards that scrolls are in view, as the table shows the
    /// view at rest.
    #[must_use]
    pub fn rows(&self) -> &Rows {
        &self.objects.rows
    }

    /// Which groups of `scroller` are in view, as the table shows the view at rest.
    #[must_use]
    pub fn row_window(&self, scroller: Scroller) -> RowWindow {
        self.objects.rows.get(scroller)
    }

    /// Scrolls `scroller` so that the first group in view, from its left end or the stack's
    /// bottom, is `first`, or the group nearest it that keeps its ends in view. Returns whether
    /// the offset changed. The caller then places every object again (`Show::scroll_row`). A
    /// line that fits never scrolls.
    pub(crate) fn scroll_row(&mut self, scroller: Scroller, first: isize) -> bool {
        let window = self.row_window(scroller);
        let first = window.clamp(first);
        if !window.scrolls() || first == window.first() {
            return false;
        }
        match scroller {
            Scroller::Row(side, row) => {
                self.scroll.battlefield[side_index(side)][row_index(row)] = first;
            }
            Scroller::Hand => self.scroll.hand = first,
            Scroller::Stack => self.scroll.stack = window.groups() - window.shown() - first,
        }
        // A card part of the way through a slide begins the next one from where it is drawn.
        let now = self.slide.now;
        let from = (self.objects.cards.iter().enumerate())
            .map(|(slot, object)| {
                let mut object = object.filter(|object| holds(scroller, object.place))?;
                self.slide_look(slot, &mut object, now);
                Some((object.pose, object.visible))
            })
            .collect();
        self.slide = Slide {
            place: Some(scroller),
            from,
            since: None,
            now,
        };
        true
    }

    /// Takes `now`, the time of the frame being drawn, and begins a slide that waits for it.
    pub(crate) fn advance_slide(&mut self, now: u64) {
        self.advance_tray(now);
        self.advance_shown(now);
        self.slide.now = now;
        if self.slide.place.is_some() && self.slide.since.is_none() {
            self.slide.since = Some(now);
        }
    }

    /// Whether a card slides or fades in at `now`.
    #[must_use]
    pub fn sliding(&self, now: u64) -> bool {
        self.slide.share(now).is_some()
    }

    /// Changes `object`, the card in `slot` as it rests, for the slide under way at `now`: a
    /// card that was drawn before the row scrolled is drawn part of the way from where it was
    /// to where it rests, and one that was not fades in where it rests. Returns whether it
    /// slides or fades.
    pub(in crate::table) fn slide_look(
        &self,
        slot: usize,
        object: &mut SceneObject,
        now: u64,
    ) -> bool {
        let slide = &self.slide;
        let (Some(share), Some((pose, seen))) =
            (slide.share(now), slide.from.get(slot).copied().flatten())
        else {
            return false;
        };
        if !slide.place.is_some_and(|line| holds(line, object.place)) || !object.visible {
            return false;
        }
        object.pose = if seen {
            between(&pose, &object.pose, share)
        } else {
            Pose {
                opacity: object.pose.opacity * share,
                ..object.pose
            }
        };
        true
    }
}

/// Whether a card at `place` is one of `scroller`'s.
fn holds(scroller: Scroller, place: Place) -> bool {
    let Place::Card { zone, side, row } = place else {
        return false;
    };
    match scroller {
        Scroller::Row(on, line) => zone == ZoneName::Battlefield && side == on && row == Some(line),
        Scroller::Hand => zone == ZoneName::Hand && side == Side::You,
        Scroller::Stack => zone == ZoneName::Stack,
    }
}
