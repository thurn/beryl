//! Text layout with parley (docs/beryl-3d.md, "Text"). Only `Client::handle` lays out text: it
//! shapes each text once, breaks it into lines at the width the layout gives it, and keeps the
//! result as plain numbers, so `Client::frame` reads glyph positions and never calls parley.
//!
//! Only the bundled fonts are registered (`fonts`), and parley is built without its `system`
//! feature, so no system font is ever found or used: a character no bundled font has is drawn
//! as the font's missing-character glyph.

pub mod fonts;
pub mod glyphs;
mod write;

use std::collections::BTreeMap;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    FontContext, FontFamily, FontFamilyName, FontWeight, Layout, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty,
};

use crate::input::Rect;
use fonts::Font;
pub use glyphs::{Glyphs, key as glyph_key};
pub use write::{BlockLook, write_block};

/// How a run of text looks: its font, weight, size in logical points and line height as a
/// multiple of its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    pub font: Font,
    /// The CSS weight, from 100 to 900. The bundled text fonts are variable, so any weight is
    /// drawn from the one file.
    pub weight: u16,
    pub size: f32,
    pub line_height: f32,
}

impl TypeStyle {
    /// The style as a key that orders and compares exactly.
    fn key(self) -> StyleKey {
        StyleKey {
            font: self.font,
            weight: self.weight,
            size: self.size.to_bits(),
            line_height: self.line_height.to_bits(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StyleKey {
    font: Font,
    weight: u16,
    size: u32,
    line_height: u32,
}

/// One text broken into lines at one width, in logical points from the top left corner of the
/// box it was laid out for.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The width it was broken at, or `None` for no limit.
    pub max_width: Option<f32>,
    /// The width of its widest line and the height of all its lines.
    pub width: f32,
    pub height: f32,
    /// Each line's box: as wide as its glyphs and as tall as the line.
    pub lines: Vec<Rect>,
    pub glyphs: Vec<PlacedGlyph>,
}

/// A glyph of a laid-out text: which glyph of the text's font, and where its pen position on
/// the baseline lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub glyph: u16,
    pub x: f32,
    pub y: f32,
}

/// A text shaped once, with each width it has been broken at.
struct Shaped {
    layout: Layout<()>,
    breaks: Vec<Block>,
    /// Whether the current layout pass has used it; a pass drops those it has not.
    used: bool,
}

/// The fonts, parley's scratch space and the shaped texts.
pub struct Text {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    /// Each bundled font's family name, as parley knows it, in the order of [`Font`].
    families: Vec<String>,
    shaped: BTreeMap<(String, StyleKey), Shaped>,
    /// Texts shaped since the counter was last taken.
    runs: u32,
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl Text {
    /// A font context holding only the bundled fonts, with system fonts switched off.
    #[must_use]
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let families = (fonts::ALL.iter())
            .map(|file| {
                let registered = collection.register_fonts(Blob::new(file_data(file.data)), None);
                let (family, _) = registered
                    .first()
                    .unwrap_or_else(|| panic!("{} holds a font", file.file));
                collection
                    .family_name(*family)
                    .expect("a registered family has a name")
                    .to_owned()
            })
            .collect();
        Self {
            fonts: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layouts: LayoutContext::new(),
            families,
            shaped: BTreeMap::new(),
            runs: 0,
        }
    }

    /// How large `text` in `style` is when broken at `max_width`, or on one line with `None`.
    pub fn measure(&mut self, text: &str, style: TypeStyle, max_width: Option<f32>) -> (f32, f32) {
        let block = self.block(text, style, max_width);
        (block.width, block.height)
    }

    /// `text` in `style` broken into lines at `max_width`. It is shaped the first time it is
    /// asked for, and each width's lines are kept, so asking again does no work.
    pub fn block(&mut self, text: &str, style: TypeStyle, max_width: Option<f32>) -> &Block {
        let key = (text.to_owned(), style.key());
        if !self.shaped.contains_key(&key) {
            let layout = self.shape(text, style);
            self.shaped.insert(
                key.clone(),
                Shaped {
                    layout,
                    breaks: Vec::new(),
                    used: false,
                },
            );
        }
        let shaped = self.shaped.get_mut(&key).expect("inserted above");
        shaped.used = true;
        let found = shaped
            .breaks
            .iter()
            .position(|block| block.max_width == max_width);
        let at = found.unwrap_or_else(|| {
            shaped.layout.break_all_lines(max_width);
            shaped.breaks.push(block_of(&shaped.layout, max_width));
            shaped.breaks.len() - 1
        });
        &shaped.breaks[at]
    }

    /// Drops every text the pass since the last call has not used, and returns how many texts
    /// were shaped since then.
    pub fn end_pass(&mut self) -> u32 {
        self.shaped
            .retain(|_, shaped| std::mem::take(&mut shaped.used));
        std::mem::take(&mut self.runs)
    }

    /// Returns how many texts were shaped since the last call, and drops none.
    pub fn take_runs(&mut self) -> u32 {
        std::mem::take(&mut self.runs)
    }

    fn shape(&mut self, text: &str, style: TypeStyle) -> Layout<()> {
        self.runs += 1;
        let family = self.families[style.font as usize].as_str();
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
            FontFamilyName::named(family),
        )));
        builder.push_default(StyleProperty::FontSize(style.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
            style.weight,
        ))));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        )));
        builder.build(text)
    }
}

/// The lines and glyphs of a layout already broken at `max_width`.
fn block_of(layout: &Layout<()>, max_width: Option<f32>) -> Block {
    let mut block = Block {
        max_width,
        width: 0.0,
        height: layout.height(),
        lines: Vec::new(),
        glyphs: Vec::new(),
    };
    for line in layout.lines() {
        let metrics = line.metrics();
        let width = metrics.advance - metrics.hanging_advance;
        block.width = block.width.max(width);
        block.lines.push(Rect {
            x: metrics.offset,
            y: metrics.block_min_coord,
            width,
            height: metrics.block_max_coord - metrics.block_min_coord,
        });
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                block
                    .glyphs
                    .extend(run.positioned_glyphs().map(|glyph| PlacedGlyph {
                        glyph: u16::try_from(glyph.id).expect("an OpenType glyph id fits 16 bits"),
                        x: glyph.x,
                        y: glyph.y,
                    }));
            }
        }
    }
    block
}

/// A bundled font's bytes as the shared buffer fontique reads.
fn file_data(data: &'static [u8]) -> std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> {
    std::sync::Arc::new(data)
}
