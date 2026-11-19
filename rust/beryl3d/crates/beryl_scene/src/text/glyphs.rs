//! The glyph table (docs/beryl-3d.md, "Text": glyphs are drawn from an atlas). It maps a glyph
//! of a font at a weight and a size in physical pixels to where the rasterizer put it, and
//! lists each glyph laid-out text needs that is not there yet, once, for the host to hand to
//! the rasterizer. A glyph that is not there at its size is drawn from the nearest size that is
//! (`Glyphs::nearest`), and one not there at any size draws nothing. Either way nothing moves
//! when it arrives, because layout never depends on rasterization.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{Block, TypeStyle};
use crate::element::{DropLayers, Shadow};
use crate::input::{GlyphCell, GlyphKey};

/// Where a rasterized glyph is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub page: u16,
    pub rect: [u16; 4],
    pub bearing: [i16; 2],
}

#[derive(Debug, Default)]
pub struct Glyphs {
    table: BTreeMap<GlyphKey, Placed>,
    /// Every glyph ever asked for, so that each is asked for once.
    asked: BTreeSet<GlyphKey>,
    wanted: VecDeque<GlyphKey>,
    /// How many glyphs of `asked` no page holds yet.
    missing: usize,
}

impl Glyphs {
    /// Records the glyphs of an atlas page that is ready.
    pub fn add(&mut self, page: u16, cells: &[GlyphCell]) {
        for cell in cells {
            let placed = Placed {
                page,
                rect: cell.rect,
                bearing: cell.bearing,
            };
            if self.table.insert(cell.key, placed).is_none() && self.asked.contains(&cell.key) {
                self.missing -= 1;
            }
        }
    }

    /// Where the glyph `key` is, if a page holds it.
    #[must_use]
    pub fn get(&self, key: GlyphKey) -> Option<Placed> {
        self.table.get(&key).copied()
    }

    /// Where the glyph `key` is, or failing that the same glyph at the nearest size a page
    /// holds, with how many times larger `key`'s size is than that cell's. A glyph at a new size
    /// (after the display's scale changed) is drawn from its old cell, scaled, until its own
    /// page arrives.
    #[must_use]
    pub fn nearest(&self, key: GlyphKey) -> Option<(Placed, f32)> {
        if let Some(placed) = self.get(key) {
            return Some((placed, 1.0));
        }
        let sizes = GlyphKey { size: 0, ..key }..=GlyphKey {
            size: u16::MAX,
            ..key
        };
        (self.table.range(sizes))
            .filter(|(cell, _)| cell.blur == key.blur)
            .min_by_key(|(cell, _)| cell.size.abs_diff(key.size))
            .map(|(cell, placed)| (*placed, f32::from(key.size) / f32::from(cell.size.max(1))))
    }

    /// Asks for each glyph of `block`, in its run's style at `scale` physical pixels per point,
    /// that no page holds and that has not been asked for.
    pub fn want(&mut self, block: &Block, scale: f32) {
        self.want_with(block, scale, &[]);
    }

    /// `want`, and each glyph blurred for each layer of `shadows`, the drop shadows its text is
    /// drawn with (`Style::drop_shadow`, `DropLayers`).
    pub fn want_with(&mut self, block: &Block, scale: f32, shadows: &[Shadow]) {
        let layers = DropLayers::new(shadows);
        for glyph in &block.glyphs {
            let style = block.run(glyph.run).style;
            let sharp = key(style, scale, glyph.glyph);
            let blurs = layers
                .iter()
                .map(|layer| blurred(sharp, layer.sigma, scale));
            for key in std::iter::once(sharp).chain(blurs) {
                if !self.table.contains_key(&key) && self.asked.insert(key) {
                    self.wanted.push_back(key);
                    self.missing += 1;
                }
            }
        }
    }

    /// How many glyphs laid-out text has asked for that no page holds yet: each is drawn from
    /// another size of itself, or not at all. A host that delivers glyphs brings it back to
    /// zero soon after the text appears.
    #[must_use]
    pub fn missing(&self) -> usize {
        self.missing
    }

    /// The glyphs asked for since the last call, each once.
    pub fn drain_wanted(&mut self) -> impl Iterator<Item = GlyphKey> + '_ {
        self.wanted.drain(..)
    }
}

/// The key of glyph `glyph` of `style`'s font, at `style`'s size in physical pixels.
#[must_use]
pub fn key(style: TypeStyle, scale: f32, glyph: u16) -> GlyphKey {
    GlyphKey {
        font: style.font as u8,
        weight: style.weight,
        glyph,
        blur: 0,
        size: pixels(style.size * scale),
    }
}

/// `key` blurred with a Gaussian of standard deviation `sigma` logical points at `scale`, as a
/// drop shadow of it is drawn: the deviation in quarters of a physical pixel, at least one
/// quarter, at most 255.
#[must_use]
pub fn blurred(key: GlyphKey, sigma: f32, scale: f32) -> GlyphKey {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to the range of u8 first"
    )]
    let blur = (sigma * scale * 4.0).round().clamp(1.0, 255.0) as u8;
    GlyphKey { blur, ..key }
}

/// A size in physical pixels, rounded to the nearest whole pixel. A font size in pixels is
/// positive and far below 65,536.
fn pixels(size: f32) -> u16 {
    size.round() as u16
}
