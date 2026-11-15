//! Writing laid-out text into a frame: one glyph record for each glyph of a block that an atlas
//! page holds (docs/beryl-3d.md, "Text"). A glyph no page holds yet draws nothing, and nothing
//! else moves when it arrives. It allocates nothing.

use glam::Vec2;

use super::glyphs::{Glyphs, key};
use super::{Block, TypeStyle};
use crate::frame::{Band, Draw, FrameBuilder, Sprite};
use crate::input::Rect;
use crate::table::Camera;

/// How a block of text is drawn.
#[derive(Clone, Copy, Debug)]
pub struct BlockLook<'a> {
    /// The style the block was laid out in.
    pub style: TypeStyle,
    /// The color of its glyphs.
    pub tint: [u8; 4],
    /// How many physical pixels make one logical point, which picks the glyphs' size.
    pub scale: f32,
    /// Where the rasterizer put each glyph.
    pub glyphs: &'a Glyphs,
    /// The camera whose flat-layer plane the glyphs are drawn on.
    pub camera: &'a Camera,
}

/// Writes a glyph record for each glyph of `block` whose top left corner, in logical points of
/// the window, is at `origin`.
pub fn write_block(builder: &mut FrameBuilder, block: &Block, origin: Vec2, look: &BlockLook) {
    let scale = look.scale;
    for glyph in &block.glyphs {
        let Some(placed) = look.glyphs.get(key(look.style, scale, glyph.glyph)) else {
            continue;
        };
        let [_, _, width, height] = placed.rect;
        let rect = Rect {
            x: origin.x + glyph.x + f32::from(placed.bearing[0]) / scale,
            y: origin.y + glyph.y + f32::from(placed.bearing[1]) / scale,
            width: f32::from(width) / scale,
            height: f32::from(height) / scale,
        };
        builder.draw(
            Band::Layer,
            Draw::glyph(
                look.camera.layer_transform(rect),
                look.tint,
                Sprite {
                    page: placed.page,
                    rect: placed.rect,
                },
            ),
        );
    }
}
