//! The Revealed Cards pile: the faces of the cards held up for both players to read (`showcase`),
//! fanned at the top left of the screen, or on the phone in the opponent's strip, the last
//! revealed on top and each earlier one's left edge showing, as the web interface's `fan`
//! (`ui/src/stage/revealed.ts`) lays them out. The pile is placed relative to the camera, so that
//! it keeps its size and place at any window shape. Each face is a second card element beside the
//! card's own, which lies where the table has it.

use beryl_protocol::protocol::{ObjectId, ObjectView};
use glam::{Quat, Vec2};

use super::look::{Dress, Look};
use super::scene::{Table, face_of};
use super::{CARD_HEIGHT, CARD_WIDTH};
use crate::data::space::layout::phone;
use crate::data::space::revealed;
use crate::faces::Faces;
use crate::frame::{ElementId, FrameBuilder, Place, REVEALED_FACE, Role, StateBits};
use crate::light::{Open, Span};
use crate::pose::{Curve, Segment, Shape};

impl Table {
    /// Keeps `object`, the record of a card held up to be read, for the Revealed Cards pile to
    /// draw its face from once the step that holds it up plays.
    pub fn reveal(&mut self, object: &ObjectView) {
        let slot = usize::from(object.slot.0);
        if self.revealed.len() <= slot {
            self.revealed.resize(slot + 1, None);
        }
        self.revealed[slot] = Some(object.clone());
    }

    /// The record of the face the card in `slot` last showed when it was held up to be read.
    pub(crate) fn revealed(&self, slot: ObjectId) -> Option<&ObjectView> {
        self.revealed.get(usize::from(slot.0))?.as_ref()
    }

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
        // On the phone the pile lies in the opponent's strip beside their graveyard, at a
        // pile's size, where the phone leaves the third pile's place free (docs/ui-conventions.md,
        // "The phone").
        let window = self.camera().window;
        let (at, height) = if self.layout().phone.is_some() {
            let pile = phone::piles::opponent::WIDTH / CARD_WIDTH * CARD_HEIGHT;
            (self.layout().piles[1][2], pile / window.size.height)
        } else {
            (revealed::AT, revealed::HEIGHT)
        };
        for (index, (open, slot)) in lying().enumerate() {
            let Some(shown) = self.revealed(slot) else {
                continue;
            };
            #[expect(clippy::cast_precision_loss, reason = "a handful of faces")]
            let index = index as f32;
            // Each later face lies a little nearer the eye, so that it covers the one under it.
            let rest = self.camera().screen_pose(
                at + Vec2::X * step * index,
                revealed::DEPTH - index,
                Quat::IDENTITY,
                height,
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
                full: None,
                // A press goes through a face to the permanent under it, as the web's
                // Revealed Cards lie over rows on purpose (`DESKTOP_REVEALED`); the pill and
                // the viewer are how a face is opened.
                state: StateBits::PASSES,
                interactive: false,
            };
            let place = Place::Revealed;
            let id = ElementId(REVEALED_FACE + u32::from(slot.0));
            let name = shown.name.as_deref().unwrap_or("");
            self.write_look(name, (id, Role::Card, 1), place, &look, back, builder);
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
