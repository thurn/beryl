//! Writing laid-out text into a frame: a panel record for the disc of each mana symbol, then a
//! glyph record for each glyph of a block that an atlas page holds (docs/beryl-3d.md, "Text").
//! A glyph no page holds at its size is drawn from the nearest size a page holds, scaled, and
//! one no page holds at all draws nothing; nothing else moves when its page arrives. It
//! allocates nothing.

use glam::Vec2;

use super::{Band, Draw, FrameBuilder, Sprite};
use crate::atlas::GlyphKey;
use crate::geometry::{Rect, plane::ScreenPlane};
use crate::paint::{DropLayers, Shadow};
use crate::text::glyphs::{Glyphs, blurred, key};
use crate::text::{Block, PlacedGlyph};

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
    pub camera: &'a ScreenPlane,
    /// The clip rectangle the glyphs are drawn under (`FrameBuilder::clip`), or 0 for none.
    pub clip: u8,
    /// The drop shadows the glyphs cast (`Style::drop_shadow`), whose layers (`DropLayers`)
    /// are drawn under them as their blurred cells in the layer's color, moved by its offset.
    pub shadows: &'a [Shadow],
}

/// Writes the symbols and glyphs of `block` whose top left corner, in logical points of the
/// window, is at `origin`. Returns how many of its glyphs had no atlas cell at their own key,
/// each drawn from another size of itself or not at all, for the "drawn" check.
pub fn write_block(
    builder: &mut FrameBuilder,
    block: &Block,
    origin: Vec2,
    look: &BlockLook,
) -> u32 {
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
    let mut missing = 0;
    // The layers of the drop shadows, under the glyphs, bottom first.
    for layer in DropLayers::new(look.shadows).iter() {
        let tint = faded(layer.color, look.opacity);
        let offset = Vec2::new(layer.x, layer.y);
        for glyph in &block.glyphs {
            let sharp = key(block.run(glyph.run).style, look.scale, glyph.glyph);
            let wanted = blurred(sharp, layer.sigma, look.scale);
            missing += write_glyph(builder, look, wanted, origin + offset, glyph, tint);
        }
    }
    for glyph in &block.glyphs {
        let run = block.run(glyph.run);
        let key = key(run.style, look.scale, glyph.glyph);
        let tint = run
            .color
            .map_or(look.tint, |color| faded(color, look.opacity));
        missing += write_glyph(builder, look, key, origin, glyph, tint);
    }
    missing
}

/// Writes the record of glyph `glyph` from the atlas cell of `key`, with the block's top left
/// corner at `origin`, in `tint`. Returns 1 if no page holds that cell, when the glyph is drawn
/// from another size of it or not at all, and 0 otherwise.
fn write_glyph(
    builder: &mut FrameBuilder,
    look: &BlockLook,
    key: GlyphKey,
    origin: Vec2,
    glyph: &PlacedGlyph,
    tint: [u8; 4],
) -> u32 {
    let (found, missing) = match look.glyphs.get(key) {
        Some(placed) => (Some((placed, 1.0)), 0),
        None => (look.glyphs.nearest(key), 1),
    };
    let Some((placed, larger)) = found else {
        return missing;
    };
    // Physical pixels of the cell to logical points of the window.
    let points = larger / look.scale;
    let [_, _, width, height] = placed.rect;
    let rect = Rect {
        x: origin.x + glyph.x + f32::from(placed.bearing[0]) * points,
        y: origin.y + glyph.y + f32::from(placed.bearing[1]) * points,
        width: f32::from(width) * points,
        height: f32::from(height) * points,
    };
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
    missing
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
