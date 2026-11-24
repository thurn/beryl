//! What the frame records for the checks and the pointer beyond its records: the outlines of
//! marks with the things they ring, and the hand cards up out of their place in the fan.

use glam::{Mat4, Vec2, Vec3};

use super::{ABILITIES, Frame, OBJECTS, StateBits};
use kit_base::{geometry::Size, paint::Shadow, pose::Pose};

/// The most hand cards the pointer can have up out of the fan at once. One is raised at a time,
/// and a pointer swept across a hand leaves each card it crossed falling for a moment; past this
/// many, the card that started falling first lands at once.
pub const LIFTED: usize = 32;

/// The most mark outlines a frame records in `Frame::rings`: two on each card, ability and
/// player. An outline past that many is drawn but not checked.
pub const RINGS: usize = 2 * (OBJECTS + ABILITIES + 2);

/// A mark's outline as the frame draws it: the thing it rings, which mark it is, and where it
/// lies. `model` places a unit quad (-0.5 to 0.5 across) in the table's space, as a record's
/// `Draw::model` does: the box the outline is drawn round, the card's own for a card, as the web
/// interface's `.stage-mark` element is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    /// The mark node that owns this outline.
    pub element: u32,
    /// The mark that names the outline in frame records.
    pub name: &'static str,
    pub model: [f32; 12],
    /// Half the width and height, in the units of `model`'s quad, of the rectangle the
    /// outline's line runs along.
    pub line: [f32; 2],
    /// Half the width and height, in the same units, of the rectangle outside which the
    /// outline and its glow draw nothing.
    pub outer: [f32; 2],
    /// The outline's color and opacity, the opacity falling as the mark comes and goes.
    pub tint: [u8; 4],
    /// The CSS box shadows the outline is drawn with, their lengths in frames (the web
    /// interface's `--frame`, a card's width), their colors white at each shadow's own opacity,
    /// which `tint` colors. Empty for an outline drawn as a sprite (a player's).
    pub shadows: &'static [Shadow],
    /// How many frames wide the box `model` places is.
    pub frames: f32,
    /// The box's corner radius, in frames.
    pub radius: f32,
}

/// Projects the corners of a model's rectangle, with dimensions relative to its unit quad.
#[must_use]
pub fn outline_corners(m: [f32; 12], [x, y]: [f32; 2], projection: Mat4, size: Size) -> [Vec2; 4] {
    [(-x, -y), (x, -y), (x, y), (-x, y)].map(|(qx, qy)| {
        let point = Vec3::new(
            m[0] * qx + m[1] * qy + m[3],
            m[4] * qx + m[5] * qy + m[7],
            m[8] * qx + m[9] * qy + m[11],
        );
        let ndc = projection.project_point3(point);
        Vec2::new(
            f32::midpoint(ndc.x, 1.0) * size.width,
            f32::midpoint(1.0, -ndc.y) * size.height,
        )
    })
}

/// A hand card up out of its place in the fan, and the box it has resting there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lifted {
    /// The index of the card's element in `Frame::resolved`.
    pub element: u32,
    /// Its corners on screen resting in the fan, in logical points.
    pub corners: [Vec2; 4],
    /// Its depth resting in the fan.
    pub depth: f32,
    /// Its pose resting in the fan, used to sample depth across its face.
    pub pose: Option<Pose>,
    /// Whether the raised paint order belongs to this pose.
    pub raised: bool,
}

impl Frame {
    /// Exchanges each lifted card's drawn pose and paint order with its place in the fan.
    /// Called twice, it leaves the frame as it was.
    pub fn swap_lifted(&mut self) {
        if self.lifted.is_empty() {
            return;
        }
        for lifted in &mut self.lifted {
            let element = &mut self.resolved[lifted.element as usize];
            std::mem::swap(&mut element.corners, &mut lifted.corners);
            std::mem::swap(&mut element.depth, &mut lifted.depth);
            std::mem::swap(&mut element.pose, &mut lifted.pose);
            let raised = element.state.has(StateBits::RAISED);
            element.state.0 &= !StateBits::RAISED;
            if lifted.raised {
                element.state.0 |= StateBits::RAISED;
            }
            lifted.raised = raised;
        }
        self.order_nodes();
    }
}
