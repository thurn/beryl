//! The arranged table and layout boxes which the HUD can anchor to.

use beryl_protocol::protocol::Side;
use glam::Vec2;
use kit_base::geometry::Rect;
use kit_frame::frame::{Anchor, ElementId, FrameBuilder};

use super::Table;
use super::place::side_index;

/// Reserved ids for table containers that have no draw records.
const HUD_ANCHOR_IDS: u32 = 1 << 23;

/// A table container or layout box with no draw records of its own.
#[derive(Clone, Copy)]
pub enum HudAnchor {
    Life(Side),
    Pool(Side),
    BackStrip(Side),
    Middle,
    CombatBanner,
}

impl HudAnchor {
    /// The stable scene id of this layout node.
    #[must_use]
    pub fn id(self) -> ElementId {
        let index = match self {
            Self::Life(side) => side_index(side),
            Self::Pool(side) => 2 + side_index(side),
            Self::BackStrip(side) => 4 + side_index(side),
            Self::Middle => 6,
            Self::CombatBanner => 7,
        };
        #[expect(clippy::cast_possible_truncation, reason = "eight layout nodes")]
        ElementId(HUD_ANCHOR_IDS + index as u32)
    }

    /// An anchor to a point in this layout box.
    #[must_use]
    pub fn at(self, at: Vec2, align: Vec2, offset: Vec2) -> Anchor {
        Anchor::new(self.id(), at, align, offset)
    }
}

impl Table {
    /// The life orb's centre before the art's offset to its numeral.
    #[must_use]
    pub fn life_centre(&self, side: Side) -> Vec2 {
        self.camera()
            .window
            .size
            .screen(self.layout().players[side_index(side)].life)
    }

    /// The point of a pool's own box that stands at its anchor, as fractions of its width and
    /// height. A layout without a banded table puts the box's top left corner there.
    #[must_use]
    pub fn pool_align(&self) -> Vec2 {
        (self.profile().data().table.banded).map_or(Vec2::ZERO, |banded| banded.pool_align)
    }

    /// Registers the life positions for the screen layer and, while HUD floats are visible,
    /// the rest of their stationary anchors. Only rows with hidden cards need a count position.
    pub fn write_hud_anchors(&self, builder: &mut FrameBuilder<'_>, floats_visible: bool) {
        let layout = self.layout();
        let window = self.camera().window.size;
        let mut register = |node: HudAnchor, bounds| builder.anchor_node(node.id(), bounds);
        for side in [Side::You, Side::Opponent] {
            register(HudAnchor::Life(side), point(self.life_centre(side)));
        }
        if !floats_visible {
            return;
        }
        for side in [Side::You, Side::Opponent] {
            let player = layout.players[side_index(side)];
            register(HudAnchor::Pool(side), point(window.screen(player.pool)));
        }
        let middle = if let Some(bands) = layout.bands {
            for (side, back) in [
                (Side::You, bands.your_back),
                (Side::Opponent, bands.opponent_back),
            ] {
                register(
                    HudAnchor::BackStrip(side),
                    Rect {
                        x: bands.left,
                        y: back.top,
                        width: bands.right - bands.left,
                        height: back.height,
                    },
                );
            }
            Rect {
                x: 0.0,
                y: bands.middle.top,
                width: window.width,
                height: bands.middle.height,
            }
        } else {
            Rect {
                x: 0.0,
                y: 0.0,
                width: window.width,
                height: window.height,
            }
        };
        register(HudAnchor::Middle, middle);
        register(
            HudAnchor::CombatBanner,
            point(Vec2::new(
                window.width / 2.0,
                self.profile().player_centre(Side::Opponent).y,
            )),
        );
    }
}

fn point(at: Vec2) -> Rect {
    Rect {
        x: at.x,
        y: at.y,
        ..Rect::default()
    }
}
