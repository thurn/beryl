//! Card edge geometry for pile side rims.

use beryl_protocol::protocol::Side;
use glam::Vec3;
use kit_base::data::Card;
use kit_base::data::profiles::PileGeometry;

const RIM_LINE: [u8; 4] = [0xa3, 0x97, 0x87, 255];
/// The color of the opponent's rims' left edges, which face away from the light.
const RIM_SHADE: [u8; 4] = [0x6b, 0x61, 0x55, 255];

/// One side of a pile's cards whose edges show beneath its top card, in the table's space.
pub(super) struct Edge {
    /// The way out of the card across the edge.
    pub(super) normal: Vec3,
    /// The way along the edge.
    pub(super) along: Vec3,
    /// Half the card's size across the edge and along it.
    pub(super) half_across: f32,
    pub(super) half_along: f32,
    /// How wide the web's line along the edge is.
    pub(super) line: f32,
    pub(super) tint: [u8; 4],
    /// Whether the spread lines are drawn individually. The opponent's left
    /// edges step less than their line is wide, so they draw one band.
    pub(super) stripes: bool,
    /// How far behind its card the edge is drawn, so that where two edges of one card
    /// meet at a corner, the light line covers the shaded one.
    pub(super) below: f32,
}

/// The sides of `side`'s pile cards, `scale` times their printed size, whose edges show
/// beneath the top card. The web's lines are 2 pixels wide for the light one and 1 for the
/// shaded one, at the reference card width.
pub(super) fn edges(
    card: Card,
    geometry: &PileGeometry,
    side: Side,
    scale: f32,
) -> impl Iterator<Item = Edge> {
    let (width, height) = (card.width * scale, card.height * scale);
    let pixel = width / geometry.rim_card_width;
    let right = Edge {
        normal: Vec3::X,
        along: Vec3::Y,
        half_across: width / 2.0,
        half_along: height / 2.0,
        line: 2.0 * pixel,
        tint: RIM_LINE,
        stripes: true,
        below: 0.0,
    };
    let near = Edge {
        normal: Vec3::NEG_Y,
        along: Vec3::X,
        half_across: height / 2.0,
        half_along: width / 2.0,
        ..right
    };
    let left = Edge {
        normal: Vec3::NEG_X,
        line: pixel,
        tint: RIM_SHADE,
        stripes: false,
        below: card.layer * geometry.rim_shade_layer,
        ..right
    };
    let edges = match side {
        Side::You => [Some(right), None],
        Side::Opponent => [Some(near), Some(left)],
    };
    edges.into_iter().flatten()
}
