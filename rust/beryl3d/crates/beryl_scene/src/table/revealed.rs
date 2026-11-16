//! The Revealed Cards pile: the faces of the cards held up for both players to read (`showcase`),
//! fanned at the top left of the screen, the last revealed on top and each earlier one's left edge
//! showing, as the web interface's `fan` (`ui/src/stage/revealed.ts`) lays them out. The pile is
//! placed relative to the camera, so that it keeps its size and place at any window shape. Each
//! face is a second card element beside the card's own, which lies where the table has it.

use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use glam::{Quat, Vec2};

use super::look::{Dress, Look};
use super::scene::{Table, face_of};
use crate::data::space::revealed;
use crate::faces::Faces;
use crate::frame::{ElementId, FrameBuilder, Place, REVEALED_FACE, Role};
use crate::light::{Open, Span};
use crate::pose::{Curve, Segment, Shape};

impl Table {
    /// Writes the faces of the Revealed Cards pile that `dress.lit` shows, each growing into its
    /// place over the step that held it up. A face that is leaving the pile is no longer drawn.
    /// Faces come from `faces`, and backs from layer `back`.
    pub(super) fn write_revealed(
        &self,
        faces: &Faces,
        (back, dress): (u16, &Dress<'_>),
        builder: &mut FrameBuilder,
    ) {
        let lying = || dress.lit.open().iter().filter_map(face);
        let count = lying().count();
        if count == 0 {
            return;
        }
        #[expect(clippy::cast_precision_loss, reason = "a handful of faces")]
        let step = revealed::STEP.min(revealed::REACH / (count.max(2) - 1) as f32);
        for (index, (open, slot)) in lying().enumerate() {
            let Some(shown) = self.revealed(slot) else {
                continue;
            };
            #[expect(clippy::cast_precision_loss, reason = "a handful of faces")]
            let index = index as f32;
            // Each later face lies a little nearer the eye, so that it covers the one under it.
            let rest = self.camera().screen_pose(
                revealed::AT + Vec2::X * step * index,
                revealed::DEPTH - index,
                Quat::IDENTITY,
                revealed::HEIGHT,
            );
            let growing = Segment {
                shape: Shape::Grown,
                ..Segment::between(rest, rest, open.begins, Curve::LINEAR)
            };
            let look = Look {
                pose: growing.pose_at(dress.now),
                tint: [255, 255, 255],
                dissolve: 0.0,
                whiten: 0.0,
                face: face_of(faces, shown),
                state: 0,
                interactive: false,
            };
            let place = Place::Card {
                zone: ZoneName::Nowhere,
                side: Side::You,
                row: None,
            };
            let id = ElementId(REVEALED_FACE + u32::from(slot.0));
            let name = shown.name.as_deref().unwrap_or("");
            self.write_look(name, (id, Role::Card), place, &look, back, builder);
        }
    }
}

/// The open span, and its card's slot, if it is a face of the pile that is not leaving it.
fn face(open: &Open) -> Option<(&Open, ObjectId)> {
    match open.span {
        Span::Revealed { slot, .. } if open.ends.is_none() => Some((open, slot)),
        _ => None,
    }
}
