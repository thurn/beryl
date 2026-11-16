//! Scrolling a battlefield row with the pointer (docs/beryl-3d.md, "Objects and placement").
//! A row too long to show every card with its least strip uncovered shows as many groups as
//! fit, and the pointer scrolls it:
//!
//! - The wheel over a card of the row scrolls it by one group for each notch
//!   (`pointer.wheel_notch_points` of travel), toward the end the wheel turns to: turned down or
//!   to the right, toward the right end. Travel short of a notch is kept for the next turn.
//! - A drag that begins on a card of a row with cards off an end scrolls it by one group for
//!   each `pointer.scroll_step_points` the pointer travels sideways, the row following the
//!   pointer: a drag to the left brings in the groups off the right end. A drag must not begin
//!   on a card that may be picked up, and no battlefield card may be: a game's prompts pick up
//!   only the viewer's hand cards.

use beryl_protocol::protocol::{RowName, Side, ZoneName};
use glam::Vec2;

use crate::agent::hit;
use crate::data::space::pointer::{SCROLL_STEP_POINTS, WHEEL_NOTCH_POINTS};
use crate::frame::{Frame, Place, Role};
use crate::input::{Button, Pointer, PointerPhase};
use crate::table::{RowWindow, Table};

/// A row to scroll, and the group that should be the first in view.
pub type RowScroll = ((Side, RowName), isize);

/// What the pointer is doing to the battlefield's rows: a drag under way, and the wheel's travel
/// short of a notch.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowGesture {
    drag: Option<Drag>,
    /// In logical points, toward the right end of a row.
    wheel: f32,
}

/// A drag that scrolls a row: the row, where the pointer went down, and the row's first group
/// in view then.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    row: (Side, RowName),
    x: f32,
    first: usize,
}

impl RowGesture {
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
                        x: pointer.position.x,
                        first: window.first(),
                    });
                None
            }
            PointerPhase::Move => {
                let drag = self.drag?;
                let groups = ((drag.x - pointer.position.x) / SCROLL_STEP_POINTS).trunc();
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

/// The battlefield row of the card at `point` in `frame`, and which of its groups are in view.
fn row_at(frame: &Frame, table: &Table, point: Vec2) -> Option<((Side, RowName), RowWindow)> {
    let element = &frame.resolved[hit(frame, point)? as usize];
    let Place::Card {
        zone: ZoneName::Battlefield,
        side,
        row: Some(row),
    } = element.place
    else {
        return None;
    };
    (element.role == Role::Card).then(|| ((side, row), table.row_window(side, row)))
}
