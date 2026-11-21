//! Resolves anchors against the table's draw records.

use beryl_protocol::script::Subject;
use glam::Vec2;

use super::{ABILITY_IDS, Table};
use crate::frame::{ElementId, FrameBuilder};
use crate::layout::Spot;

/// Where `subject` stands on the window as the table drew it into `builder` this frame: a
/// card's or an ability's box, wherever its flight has taken it, or a player's life total's
/// place. `None` for a card or an ability the table did not draw.
#[must_use]
pub fn spot_of(builder: &FrameBuilder, table: &Table, subject: Subject) -> Option<Spot> {
    let boxed = |id: u32| {
        let corners = builder.corners(ElementId(id))?;
        let centre = corners.iter().copied().sum::<Vec2>() / 4.0;
        let highest = (corners.iter()).fold(f32::INFINITY, |high, corner| high.min(corner.y));
        let lowest = (corners.iter()).fold(f32::NEG_INFINITY, |low, corner| low.max(corner.y));
        Some(Spot {
            centre,
            top: Vec2::new(centre.x, highest),
            bottom: Vec2::new(centre.x, lowest),
        })
    };
    match subject {
        Subject::Object { slot } => boxed(u32::from(slot.0)),
        Subject::Ability { id } => boxed(ABILITY_IDS + id % ABILITY_IDS),
        Subject::Player { player } => Some(Spot::point(
            table.camera().to_screen(table.player_pose(player).position),
        )),
    }
}
