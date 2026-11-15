//! The glyph table (docs/beryl-3d.md, "Text": glyphs are drawn from an atlas). It maps a glyph
//! of a font at a weight and a size in physical pixels to where the rasterizer put it, and
//! lists each glyph laid-out text needs that is not there yet, once, for the host to hand to
//! the rasterizer. A glyph that is not there draws nothing and moves nothing, because layout
//! never depends on rasterization.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::input::{GlyphCell, GlyphKey};
use crate::text::{Block, TypeStyle};

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
}

impl Glyphs {
    /// Records the glyphs of an atlas page that is ready.
    pub fn add(&mut self, page: u16, cells: &[GlyphCell]) {
        for cell in cells {
            self.table.insert(
                cell.key,
                Placed {
                    page,
                    rect: cell.rect,
                    bearing: cell.bearing,
                },
            );
        }
    }

    /// Where the glyph `key` is, if a page holds it.
    #[must_use]
    pub fn get(&self, key: GlyphKey) -> Option<Placed> {
        self.table.get(&key).copied()
    }

    /// Asks for each glyph of `block`, in `style` at `scale` physical pixels per point, that no
    /// page holds and that has not been asked for.
    pub fn want(&mut self, block: &Block, style: TypeStyle, scale: f32) {
        for glyph in &block.glyphs {
            let key = key(style, scale, glyph.glyph);
            if !self.table.contains_key(&key) && self.asked.insert(key) {
                self.wanted.push_back(key);
            }
        }
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
        size: pixels(style.size * scale),
    }
}

/// A size in physical pixels, rounded to the nearest whole pixel. A font size in pixels is
/// positive and far below 65,536.
fn pixels(size: f32) -> u16 {
    size.round() as u16
}
