//! Scrolling a battlefield row, the viewer's hand or the stack with the pointer
//! (docs/beryl-3d.md, "Objects and placement"). A line of cards too long to show every card
//! with its least strip uncovered shows as many groups as fit, and the pointer scrolls it:
//!
//! - The wheel over a card of the line scrolls it by one group for each notch
//!   (`pointer.wheel_notch_points` of travel), toward the end the wheel turns to: turned down or
//!   to the right, toward the right end. Travel short of a notch is kept for the next turn.
//! - A drag that begins on a card of a line with cards off an end scrolls it by one group for
//!   each `pointer.scroll_step_points` the pointer travels sideways, the line following the
//!   pointer: a drag to the left brings in the groups off the right end. No battlefield card or
//!   stack item can be picked up. A hand card can, by a drag, so a drag on the hand scrolls it
//!   only when it first travels `pointer.drag_threshold_points` more sideways than up or down,
//!   which then picks no card up (`Grip::moved`): a card is played by dragging it up.

use beryl_protocol::protocol::{Side, ZoneName};
use glam::Vec2;

use game_table::table::{RowWindow, Scroller, Table};
use kit_base::data::space::pointer::{
    DRAG_THRESHOLD_POINTS, SCROLL_STEP_POINTS, WHEEL_NOTCH_POINTS,
};
use kit_frame::frame::{Frame, Place, Role};
use kit_scene::hit::hit;
use kit_ui::input::{Button, Pointer, PointerPhase};

/// A line of cards to scroll, and the group that should be the first in view.
pub type RowScroll = (Scroller, isize);

/// What the pointer is doing to the battlefield's rows: a drag under way, and the wheel's travel
/// short of a notch.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowGesture {
    drag: Option<Drag>,
    /// In logical points, toward the right end of a row.
    wheel: f32,
}

/// A drag that scrolls a line of cards: the line, where the pointer went down, and the line's
/// first group in view then.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    row: Scroller,
    start: Vec2,
    first: usize,
    /// Whether the drag has travelled far enough to scroll: at once for a row or the stack, and
    /// for the hand once it has travelled the drag threshold sideways.
    scrolling: bool,
}

impl RowGesture {
    /// Ends the drag under way, as when a second finger turns a touch into a multi-finger tap.
    pub fn cancel(&mut self) {
        self.drag = None;
    }

    /// Takes a pointer event, with `frame`, the last frame drawn, to find what is under the
    /// pointer, and `table` for which groups of each row are in view. Returns the row to scroll,
    /// if the event scrolls one.
    pub fn handle(&mut self, pointer: &Pointer, frame: &Frame, table: &Table) -> Option<RowScroll> {
        match pointer.phase {
            PointerPhase::Wheel => {
                let (row, window) = row_at(frame, table, pointer.position)?;
                // winit's sign: a positive delta moves the content right and down, which a
                // wheel turned up or to the left does.
                self.wheel -= pointer.wheel.x + pointer.wheel.y;
                let notches = (self.wheel / WHEEL_NOTCH_POINTS).trunc();
                self.wheel -= notches * WHEEL_NOTCH_POINTS;
                (notches != 0.0).then_some((row, window.first() as isize + notches as isize))
            }
            PointerPhase::Down if pointer.button == Button::Primary => {
                self.drag = row_at(frame, table, pointer.position)
                    .filter(|(_, window)| window.scrolls())
                    .map(|(row, window)| Drag {
                        row,
                        start: pointer.position,
                        first: window.first(),
                        scrolling: row != Scroller::Hand,
                    });
                None
            }
            PointerPhase::Move => {
                let drag = self.drag.as_mut()?;
                if !drag.scrolling {
                    match sideways(drag.start, pointer.position) {
                        None => return None,
                        Some(false) => {
                            self.drag = None;
                            return None;
                        }
                        Some(true) => drag.scrolling = true,
                    }
                }
                let groups = ((drag.start.x - pointer.position.x) / SCROLL_STEP_POINTS).trunc();
                Some((drag.row, drag.first as isize + groups as isize))
            }
            PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave => {
                self.drag = None;
                None
            }
            PointerPhase::Down => None,
        }
    }
}

/// Whether a drag from `start` to `at` goes sideways, once it has travelled the drag threshold:
/// farther across than up or down. `None` while it is short of the threshold.
pub(super) fn sideways(start: Vec2, at: Vec2) -> Option<bool> {
    let travel = at - start;
    (travel.length() >= DRAG_THRESHOLD_POINTS).then(|| travel.x.abs() > travel.y.abs())
}

/// The line of cards that scrolls which the card at `point` in `frame` stands in, and which of
/// its groups are in view.
fn row_at(frame: &Frame, table: &Table, point: Vec2) -> Option<(Scroller, RowWindow)> {
    let element = &frame.resolved[hit(frame, point)? as usize];
    let scroller = scroller_of(element.place)?;
    (element.role == Role::Card).then(|| (scroller, table.row_window(scroller)))
}

/// The line of cards that scrolls which a card at `place` stands in, if any.
#[must_use]
pub fn scroller_of(place: Place) -> Option<Scroller> {
    let Place::Card { zone, side, row } = place else {
        return None;
    };
    match (zone, row) {
        (ZoneName::Battlefield, Some(row)) => Some(Scroller::Row(side, row)),
        (ZoneName::Hand, _) if side == Side::You => Some(Scroller::Hand),
        (ZoneName::Stack, _) => Some(Scroller::Stack),
        _ => None,
    }
}
