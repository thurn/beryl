//! Empty-space drop regions fitted to the hand and battlefield layout.

use beryl_protocol::protocol::{RowName, Side};
use glam::{Vec2, Vec3};
use kit_base::geometry::Rect;
use kit_frame::frame::{
    AnchorBox, ElementId, FrameBuilder,
    picking::{Area, DropRegion},
};

use super::place::{row_index, side_index};
use super::{Camera, Table};

/// The empty-space destinations on a manual table.
#[derive(Clone, Copy)]
pub enum DropZone {
    Hand(Side),
    Row(Side, RowName),
}

// Keep the drop regions apart from the HUD's layout ids.
const DROP_REGION_IDS: u32 = 1 << 24;

impl DropZone {
    /// The stable id registered in the frame for this zone.
    #[must_use]
    pub fn id(self) -> ElementId {
        let index = match self {
            Self::Hand(side) => side_index(side),
            Self::Row(side, row) => 2 + side_index(side) * 3 + row_index(row),
        };
        #[expect(clippy::cast_possible_truncation, reason = "eight drop regions")]
        ElementId(DROP_REGION_IDS + index as u32)
    }

    /// The manual destination declared by this scene region.
    #[must_use]
    pub fn from_id(id: ElementId) -> Option<Self> {
        [Side::You, Side::Opponent]
            .into_iter()
            .flat_map(|side| {
                [Self::Hand(side)]
                    .into_iter()
                    .chain(super::ROWS.map(|row| Self::Row(side, row)))
            })
            .find(|zone| zone.id() == id)
    }
}

impl Table {
    /// Registers the hands and rows even when they contain no cards. A hand wins at its
    /// edge, and the back row and other-permanents half win at their split lines.
    pub fn write_drop_regions(&self, frame: &mut FrameBuilder<'_>) {
        let layout = self.layout();
        let camera = self.camera();
        let window = camera.window.size;
        let hand_top = f32::midpoint(1.0, -layout.hand.edge) * window.height;
        let hand_bottom = self.data().flat.manual.drop.opponent_hand * window.height;
        let band = |top, bottom| Rect {
            x: 0.0,
            y: top,
            width: window.width,
            height: bottom - top,
        };
        let mut register = |zone: DropZone, bounds, area| {
            frame.drop_region(DropRegion {
                id: zone.id(),
                bounds,
                area,
            });
        };
        for (side, bounds) in [
            (Side::You, band(hand_top, window.height)),
            (Side::Opponent, band(0.0, hand_bottom)),
        ] {
            register(DropZone::Hand(side), bounds, Area::Rect(bounds, None));
        }
        if hand_bottom >= hand_top {
            return;
        }

        let near = layout.creatures.y.abs();
        let back = layout.lands.y.abs();
        let split = f32::midpoint(near, back);
        let far = 2.0 * back - split;
        let middle = f32::midpoint(layout.lands.right, layout.others.left);
        let [left, right] = [layout.creatures.left, layout.creatures.right];
        let project = |x, y| camera.to_screen(Vec3::new(x, y, 0.0));
        let centre = project(middle, 0.0);
        let [you, opponent] = [-split, split].map(|y| project(middle, y));
        let clip_y = |y: f32| y.clamp(hand_bottom, hand_top);

        for (side, sign, split_at, edge) in [
            (Side::Opponent, 1.0, opponent, hand_bottom),
            (Side::You, -1.0, you, hand_top),
        ] {
            let y = clip_y(split_at.y);
            let centre_y = clip_y(centre.y);
            let back_band = band(y.min(edge), y.max(edge));
            let front_band = band(y.min(centre_y), y.max(centre_y));
            // Perspective makes the divider between lands and other permanents slant.
            // Extend both hit shapes to the window's edge.
            let delta = split_at - centre;
            let at_y = |y| centre.x + (y - centre.y) * (delta.x / delta.y);
            let split_x = at_y(y);
            let edge_x = at_y(edge);
            for (row, across, outer) in [
                (
                    RowName::Others,
                    [middle, right],
                    window.width.max(split_x).max(edge_x),
                ),
                (
                    RowName::Lands,
                    [left, middle],
                    0.0_f32.min(split_x).min(edge_x),
                ),
            ] {
                let corners = [
                    Vec2::new(split_x, y),
                    Vec2::new(outer, y),
                    Vec2::new(outer, edge),
                    Vec2::new(edge_x, edge),
                ];
                let bounds = row_bounds(
                    camera,
                    across,
                    [sign * split, sign * far],
                    (hand_bottom, hand_top),
                );
                register(
                    DropZone::Row(side, row),
                    bounds,
                    Area::Quad(corners, Some(back_band)),
                );
            }
            let bounds = row_bounds(
                camera,
                [left, right],
                [0.0, sign * split],
                (hand_bottom, hand_top),
            );
            register(
                DropZone::Row(side, RowName::Creatures),
                bounds,
                Area::Rect(front_band, None),
            );
        }
    }
}

fn row_bounds(
    camera: &Camera,
    [left, right]: [f32; 2],
    [near, far]: [f32; 2],
    hands: (f32, f32),
) -> Rect {
    let corners = [(left, near), (right, near), (right, far), (left, far)]
        .map(|(x, y)| camera.to_screen(Vec3::new(x, y, 0.0)));
    let mut bounds = AnchorBox::around(corners).rect();
    let top = bounds.y.max(hands.0);
    let bottom = (bounds.y + bounds.height).min(hands.1);
    bounds.y = top;
    bounds.height = (bottom - top).max(0.0);
    bounds
}
