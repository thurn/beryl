//! Maps session subjects to HUD anchors and resolves them against the current frame.

use beryl_protocol::script::Subject;
use glam::Vec2;

use game_table::table::{ABILITY_IDS, HudAnchor};
use kit_frame::frame::{AnchorBox, ElementId, FrameBuilder, NodeRef};
use kit_ui::{element::AnchorTarget, layout::Spot};

/// Maps a session subject to its drawn scene node or registered life anchor.
#[must_use]
pub fn anchor_target(subject: Subject) -> AnchorTarget {
    match subject {
        Subject::Object { slot } => AnchorTarget::Node(ElementId(u32::from(slot.0))),
        Subject::Ability { id } => AnchorTarget::Node(ElementId(ABILITY_IDS + id % ABILITY_IDS)),
        Subject::Player { player } => AnchorTarget::Layout(HudAnchor::Life(player).id()),
    }
}

/// Where `target` stands on the window as the table drew it into `builder` this frame: a
/// card's or an ability's box, wherever its flight has taken it, or a player's life total's
/// place. `None` if the object was not drawn or the layout node was not registered.
#[must_use]
pub fn spot_of(builder: &FrameBuilder, target: AnchorTarget) -> Option<Spot> {
    match target {
        AnchorTarget::Node(id) => {
            let corners = builder.corners(id)?;
            let centre = corners.iter().copied().sum::<Vec2>() / corners.len() as f32;
            let highest = (corners.iter()).fold(f32::INFINITY, |high, corner| high.min(corner.y));
            let lowest = (corners.iter()).fold(f32::NEG_INFINITY, |low, corner| low.max(corner.y));
            Some(Spot {
                centre,
                top: Vec2::new(centre.x, highest),
                bottom: Vec2::new(centre.x, lowest),
            })
        }
        AnchorTarget::Layout(id) => {
            let bounds = AnchorBox::from(builder.anchor_bounds(&NodeRef::Id(id))?);
            let vertical = Vec2::Y * bounds.half.y;
            Some(Spot {
                centre: bounds.centre,
                top: bounds.centre - vertical,
                bottom: bounds.centre + vertical,
            })
        }
    }
}
