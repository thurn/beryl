//! Writing laid-out text into a frame: a panel record for the disc of each mana symbol, then a
//! glyph record for each glyph of a block that an atlas page holds (docs/beryl-3d.md, "Text").
//! A glyph no page holds at its size is drawn from the nearest size a page holds, scaled, and
//! one no page holds at all draws nothing; nothing else moves when its page arrives. It
//! allocates nothing.

use glam::Vec2;

use super::Block;
use super::glyphs::{Glyphs, key};
use crate::frame::{Band, Draw, FrameBuilder, Sprite};
use crate::input::Rect;
use crate::table::Camera;

/// How a block of text is drawn.
#[derive(Clone, Copy, Debug)]
pub struct BlockLook<'a> {
    /// The color of the glyphs whose run has no color of its own.
    pub tint: [u8; 4],
    /// How opaque the block is, from 0 to 1, which fades the colors of its runs and symbols.
    /// `tint` is already faded.
    pub opacity: f32,
    /// How many physical pixels make one logical point, which picks the glyphs' size.
    pub scale: f32,
    /// Where the rasterizer put each glyph.
    pub glyphs: &'a Glyphs,
    /// The camera whose flat-layer plane the glyphs are drawn on.
    pub camera: &'a Camera,
    /// The clip rectangle the glyphs are drawn under (`FrameBuilder::clip`), or 0 for none.
    pub clip: u8,
}

/// Writes the symbols and glyphs of `block` whose top left corner, in logical points of the
/// window, is at `origin`.
pub fn write_block(builder: &mut FrameBuilder, block: &Block, origin: Vec2, look: &BlockLook) {
    for symbol in &block.symbols {
        let rect = Rect {
            x: origin.x + symbol.rect.x,
            y: origin.y + symbol.rect.y,
            ..symbol.rect
        };
        let mut draw = Draw::panel(
            look.camera.layer_transform(rect),
            faded(symbol.color, look.opacity),
            [0; 4],
            Vec2::new(rect.width, rect.height),
            rect.width / 2.0,
            0.0,
        );
        if look.clip > 0 {
            draw = draw.with_clip(look.clip);
        }
        builder.draw(Band::Layer, draw);
    }
    let scale = look.scale;
    for glyph in &block.glyphs {
        let run = block.run(glyph.run);
        let Some((placed, larger)) = look.glyphs.nearest(key(run.style, scale, glyph.glyph)) else {
            continue;
        };
        // Physical pixels of the cell to logical points of the window.
        let points = larger / scale;
        let [_, _, width, height] = placed.rect;
        let rect = Rect {
            x: origin.x + glyph.x + f32::from(placed.bearing[0]) * points,
            y: origin.y + glyph.y + f32::from(placed.bearing[1]) * points,
            width: f32::from(width) * points,
            height: f32::from(height) * points,
        };
        let tint = run
            .color
            .map_or(look.tint, |color| faded(color, look.opacity));
        let mut draw = Draw::glyph(
            look.camera.layer_transform(rect),
            tint,
            Sprite {
                page: placed.page,
                rect: placed.rect,
            },
        );
        if look.clip > 0 {
            draw = draw.with_clip(look.clip);
        }
        builder.draw(Band::Layer, draw);
    }
}

/// `color` with its alpha scaled by `opacity`.
pub(crate) fn faded(color: [u8; 4], opacity: f32) -> [u8; 4] {
    let [red, green, blue, alpha] = color;
    // An alpha times a value from 0 to 1 stays in 0 to 255, so the cast loses nothing but the
    // fraction.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the product is in 0 to 255"
    )]
    let alpha = (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}
