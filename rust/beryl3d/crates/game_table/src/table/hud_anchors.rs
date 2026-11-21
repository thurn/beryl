//! The arranged table and layout boxes which the HUD can anchor to.

use beryl_protocol::protocol::Side;
use glam::{Vec2, Vec3};
use kit_base::data::profiles::Piece;
use kit_base::geometry::Rect;
use kit_base::tokens::{size, type_style};
use kit_frame::frame::{Anchor, ElementId, FrameBuilder};

use super::place::{row_index, side_index};
use super::{ROWS, Scroller, Table, row_area};

/// Reserved ids for table containers that have no draw records.
const HUD_ANCHOR_IDS: u32 = 1 << 23;

/// A table container or layout box with no draw records of its own.
#[derive(Clone, Copy)]
pub enum HudAnchor {
    Life(Side),
    Pool(Side),
    BackStrip(Side),
    Row(Scroller),
    Middle,
    CombatBanner,
}

impl HudAnchor {
    fn id(self) -> ElementId {
        let index = match self {
            Self::Life(side) => side_index(side),
            Self::Pool(side) => 2 + side_index(side),
            Self::BackStrip(side) => 4 + side_index(side),
            Self::Row(Scroller::Row(side, row)) => 6 + side_index(side) * 3 + row_index(row),
            Self::Row(Scroller::Hand) => 12,
            Self::Row(Scroller::Stack) => 13,
            Self::Middle => 14,
            Self::CombatBanner => 15,
        };
        #[expect(clippy::cast_possible_truncation, reason = "sixteen layout nodes")]
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

    /// Registers the stationary layout boxes read by the HUD anchor pass. Only rows
    /// with hidden cards need a count position.
    pub fn write_hud_anchors(&self, builder: &mut FrameBuilder<'_>) {
        let layout = self.layout();
        let window = self.camera().window.size;
        let mut register = |node: HudAnchor, bounds| builder.anchor_node(node.id(), bounds);
        for side in [Side::You, Side::Opponent] {
            let player = layout.players[side_index(side)];
            register(HudAnchor::Life(side), point(self.life_centre(side)));
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
        let rows = [Side::You, Side::Opponent]
            .into_iter()
            .flat_map(|side| ROWS.map(|row| Scroller::Row(side, row)));
        for scroller in rows.chain([Scroller::Hand, Scroller::Stack]) {
            let row = self.rows().get(scroller);
            if (row.before() > 0 || row.after() > 0)
                && let Some(bounds) = self.count_rail(scroller)
            {
                register(HudAnchor::Row(scroller), bounds);
            }
        }
    }

    fn count_rail(&self, scroller: Scroller) -> Option<Rect> {
        let layout = self.layout();
        let camera = self.camera();
        let window = camera.window.size;
        let bounds = match scroller {
            Scroller::Row(side, row) => {
                let (area, y) = row_area((layout, camera), side, row);
                let [low, high] =
                    [area.left, area.right].map(|x| camera.to_screen(Vec3::new(x, y, 0.0)));
                let (mut left, mut right) = (low.x, high.x);
                if self.profile().data().hud.table.contains(&Piece::StepStops) {
                    // Leave room for the life orb and step stops when the count is level with them.
                    let life = window.screen(layout.players[side_index(side)].life);
                    let half_orb = size::ORB_HEIGHT * layout.play.life.orb_scale / 2.0;
                    let half_line = type_style::BODY.size * type_style::BODY.line_height / 2.0;
                    if (low.y - life.y).abs() < half_orb + half_line {
                        left = left.min(life.x - size::STEPS_WIDTH / 2.0);
                        right = right.max(life.x + size::STEPS_WIDTH / 2.0);
                    }
                }
                Rect {
                    x: left,
                    y: low.y,
                    width: right - left,
                    height: 0.0,
                }
            }
            Scroller::Stack => {
                let (low, high) = self.stack_bounds()?;
                Rect {
                    x: low.x,
                    y: low.y,
                    width: high.x - low.x,
                    height: high.y - low.y,
                }
            }
            Scroller::Hand => {
                let (left, right) = layout
                    .bands
                    .map_or((0.0, window.width), |bands| (bands.left, bands.right));
                Rect {
                    x: left,
                    y: (1.0 - layout.hand.edge) / 2.0 * window.height,
                    width: right - left,
                    height: 0.0,
                }
            }
        };
        Some(bounds)
    }
}

fn point(at: Vec2) -> Rect {
    Rect {
        x: at.x,
        y: at.y,
        ..Rect::default()
    }
}
