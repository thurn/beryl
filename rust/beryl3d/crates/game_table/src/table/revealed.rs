//! The Revealed Cards pile: the faces of the cards held up for both players to read (`showcase`),
//! fanned at the top left of the screen, or on the phone in the opponent's strip, the last
//! revealed on top and each earlier one's left edge showing, as the web interface's `fan`
//! (`ui/src/stage/revealed.ts`) lays them out. The pile is placed relative to the camera, so that
//! it keeps its size and place at any window shape. Each face is a second card element beside the
//! card's own, which lies where the table has it.

use beryl_protocol::protocol::{ObjectId, ObjectView};
use glam::{Quat, Vec3};

use super::look::Look;
use super::scene::{Table, face_of};
use crate::faces::Faces;
use kit_base::data::space::revealed;
use kit_base::pose::{Curve, Interval, Segment, Shape};
use kit_frame::frame::{ElementId, FrameBuilder, Place, REVEALED_FACE, Role, StateBits};
use kit_space::arrange::Pile;

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
    pub fn revealed(&self, slot: ObjectId) -> Option<&ObjectView> {
        self.revealed.get(usize::from(slot.0))?.as_ref()
    }

    /// Writes the visible faces of the Revealed Cards pile at `now`, each growing into its
    /// place over the interval that held it up.
    /// Faces come from `faces`, and backs from layer `back`.
    pub(super) fn write_revealed(
        &self,
        faces: &Faces,
        (back, now): (u16, u64),
        revealed: impl Iterator<Item = (ObjectId, Interval)> + Clone,
        builder: &mut FrameBuilder,
    ) {
        let count = revealed.clone().count();
        if count == 0 {
            return;
        }
        // Later faces lie nearer the eye so they cover the ones underneath. The x and y
        // offsets are in normalized device coordinates, and depth is in millimetres.
        let pile = Pile {
            step: Vec3::new(revealed::STEP, 0.0, -1.0),
        }
        .limited(count, revealed::REACH);
        let (at, height) = self.layout().revealed;
        for (index, (slot, begins)) in revealed.enumerate() {
            let Some(shown) = self.revealed(slot) else {
                continue;
            };
            let offset = pile.offset(index);
            let rest = self.camera().screen_pose(
                at + offset.truncate(),
                revealed::DEPTH + offset.z,
                Quat::IDENTITY,
                height,
            );
            let growing = Segment {
                shape: Shape::Grown(self.data().verbs.showcase),
                ..Segment::between(rest, rest, begins, Curve::LINEAR)
            };
            let look = Look {
                pose: growing.pose_at(now),
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
