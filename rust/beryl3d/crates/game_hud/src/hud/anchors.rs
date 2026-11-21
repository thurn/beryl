//! Maps session subjects to HUD anchors and resolves them against the table's draw records.

use beryl_protocol::{protocol::Side, script::Subject};
use glam::Vec2;

use game_table::table::{ABILITY_IDS, Table};
use kit_frame::frame::{ElementId, FrameBuilder};
use kit_ui::{element::AnchorTarget, layout::Spot};

/// Maps a session subject to its projected scene node or a seat's layout slot.
#[must_use]
pub fn anchor_target(subject: Subject) -> AnchorTarget {
    match subject {
        Subject::Object { slot } => AnchorTarget::Node(ElementId(u32::from(slot.0))),
        Subject::Ability { id } => AnchorTarget::Node(ElementId(ABILITY_IDS + id % ABILITY_IDS)),
        Subject::Player { player } => AnchorTarget::Slot(match player {
            Side::You => 0,
            Side::Opponent => 1,
        }),
    }
}

/// Where `target` stands on the window as the table drew it into `builder` this frame: a
/// card's or an ability's box, wherever its flight has taken it, or a player's life total's
/// place. `None` for a card or an ability the table did not draw.
#[must_use]
pub fn spot_of(builder: &FrameBuilder, table: &Table, target: AnchorTarget) -> Option<Spot> {
    let boxed = |id: ElementId| {
        let corners = builder.corners(id)?;
        let centre = corners.iter().copied().sum::<Vec2>() / corners.len() as f32;
        let highest = (corners.iter()).fold(f32::INFINITY, |high, corner| high.min(corner.y));
        let lowest = (corners.iter()).fold(f32::NEG_INFINITY, |low, corner| low.max(corner.y));
        Some(Spot {
            centre,
            top: Vec2::new(centre.x, highest),
            bottom: Vec2::new(centre.x, lowest),
        })
    };
    match target {
        AnchorTarget::Node(id) => boxed(id),
        AnchorTarget::Slot(seat) => {
            let side = match seat {
                0 => Side::You,
                1 => Side::Opponent,
                _ => return None,
            };
            Some(Spot::point(
                table.camera().to_screen(table.player_pose(side).position),
            ))
        }
    }
}
