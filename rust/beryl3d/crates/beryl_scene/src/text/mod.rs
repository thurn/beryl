//! Text layout with parley (docs/beryl-3d.md, "Text"). Only `Client::handle` lays out text: it
//! shapes each text once, breaks it into lines at the width the layout gives it, and keeps the
//! result as plain numbers, so `Client::frame` reads glyph positions and never calls parley.
//!
//! Only the bundled fonts are registered (`fonts`), and parley is built without its `system`
//! feature, so no system font is ever found or used: a character no bundled font has is drawn
//! as the font's missing-character glyph.

pub mod fonts;
pub mod glyphs;
mod symbols;
mod write;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    FontContext, FontFamily, FontFamilyName, FontWeight, InlineBox, InlineBoxKind, Layout,
    LayoutContext, LineHeight, PlainEditor, PositionedLayoutItem, StyleProperty, VerticalAlign,
};

use crate::element::Span;
use crate::input::Rect;
use fonts::Font;
pub use glyphs::{Glyphs, key as glyph_key};
pub(crate) use write::faded;
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

/// A styled span as a key that orders and compares exactly.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SpanKey {
    start: u32,
    end: u32,
    style: StyleKey,
    color: Option<[u8; 4]>,
}

/// What a laid-out text is cached by: its words, its style and its spans.
type TextKey = (String, StyleKey, Vec<SpanKey>);

/// One text broken into lines at one width, in logical points from the top left corner of the
/// box it was laid out for.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The width it was broken at, or `None` for no limit.
    pub max_width: Option<f32>,
    /// The width of its widest line and the height of all its lines.
    pub width: f32,
    pub height: f32,
    /// Each line: its box, its part of the text and its baseline.
    pub lines: Vec<Line>,
    /// How the glyphs of run 0 look: the text's own style.
    pub base: Run,
    /// How the glyphs of each later run look, from run 1: each styled span's, then one for the
    /// glyph of each mana symbol. Plain text has none, and allocates nothing for them.
    pub runs: Vec<Run>,
    pub glyphs: Vec<PlacedGlyph>,
    /// The disc of each mana symbol, drawn under its glyph.
    pub symbols: Vec<Symbol>,
}

impl Block {
    /// The look of the glyphs of run `index`.
    #[must_use]
    pub fn run(&self, index: u16) -> &Run {
        match index {
            0 => &self.base,
            later => &self.runs[usize::from(later) - 1],
        }
    }
}

/// One laid-out line of a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// Its box: as wide as its glyphs and as tall as the line.
    pub rect: Rect,
    /// The byte range of the line in the text the block was shaped from, with each mana symbol
    /// as one object character (`shaped_words`).
    pub start: u32,
    pub end: u32,
    /// The baseline's distance below the block's top, in logical points.
    pub baseline: f32,
}

impl Line {
    /// The line's words in `words`, the text its block was shaped from as `shaped_words`
    /// writes it; empty when `words` is not that text.
    #[must_use]
    pub fn words<'a>(&self, words: &'a str) -> &'a str {
        words
            .get(self.start as usize..self.end as usize)
            .unwrap_or("")
    }
}

/// `text` as it is shaped: each `{...}` mana symbol in it replaced by one object character
/// (U+FFFC), which the symbol's disc stands in for.
#[must_use]
pub fn shaped_words(text: &str) -> Cow<'_, str> {
    words_of(text, &symbols::tokens(text), &mut Vec::new())
}

/// `text` with each of its symbol `tokens` replaced by one object character, as
/// `shaped_words` writes it, putting where each object character lands in `objects`.
fn words_of<'a>(text: &'a str, tokens: &[Range<usize>], objects: &mut Vec<usize>) -> Cow<'a, str> {
    if tokens.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut words = String::with_capacity(text.len());
    let mut from = 0;
    for token in tokens {
        words.push_str(&text[from..token.start]);
        objects.push(words.len());
        words.push(symbols::OBJECT);
        from = token.end;
    }
    words.push_str(&text[from..]);
    Cow::Owned(words)
}

/// The look of some of a block's glyphs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub style: TypeStyle,
    /// Their color, or `None` for the color of the element that shows the text.
    pub color: Option<[u8; 4]>,
}

/// A glyph of a laid-out text: which glyph of its run's font, the run it belongs to, and where
/// its pen position on the baseline lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub glyph: u16,
    /// Its run, which [`Block::run`] gives the look of.
    pub run: u16,
    pub x: f32,
    pub y: f32,
}

/// The disc a mana symbol is drawn on: an inline box the size of its line's em.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Symbol {
    pub rect: Rect,
    pub color: [u8; 4],
}

/// A mana symbol of a shaped text, before the text is broken into lines: its disc's color and
/// its glyph, laid out in a box `em` wide with its top left corner at the origin.
struct ShapedSymbol {
    color: [u8; 4],
    em: f32,
    glyphs: Vec<PlacedGlyph>,
}

/// A text shaped once, with each width it has been broken at.
struct Shaped {
    layout: Layout<u16>,
    base: Run,
    runs: Vec<Run>,
    symbols: Vec<ShapedSymbol>,
    breaks: Vec<Block>,
    /// Whether the current layout pass has used it; a pass drops those it has not.
    used: bool,
}

/// The fonts, parley's scratch space and the shaped texts.
pub struct Text {
    fonts: FontContext,
    layouts: LayoutContext<u16>,
    /// Each bundled font's family name, as parley knows it, in the order of [`Font`].
    families: Vec<String>,
    shaped: BTreeMap<TextKey, Shaped>,
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

    /// How large `text` in `style`, with `spans` styled apart, is when broken at `max_width`,
    /// or on one line with `None`.
    pub fn measure(
        &mut self,
        text: &str,
        style: TypeStyle,
        spans: &[Span],
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let block = self.block(text, style, spans, max_width);
        (block.width, block.height)
    }

    /// `text` in `style`, with `spans` styled apart, broken into lines at `max_width`. Each
    /// `{...}` in it is a mana symbol (`symbols`). It is shaped the first time it is asked for,
    /// and each width's lines are kept, so asking again does no work.
    pub fn block(
        &mut self,
        text: &str,
        style: TypeStyle,
        spans: &[Span],
        max_width: Option<f32>,
    ) -> &Block {
        let key = (
            text.to_owned(),
            style.key(),
            (spans.iter())
                .map(|span| SpanKey {
                    start: span.range.start,
                    end: span.range.end,
                    style: span.style.key(),
                    color: span.color,
                })
                .collect(),
        );
        if !self.shaped.contains_key(&key) {
            let shaped = self.shape(text, style, spans);
            self.shaped.insert(key.clone(), shaped);
        }
        let shaped = self.shaped.get_mut(&key).expect("inserted above");
        shaped.used = true;
        let found = shaped
            .breaks
            .iter()
            .position(|block| block.max_width == max_width);
        let at = found.unwrap_or_else(|| {
            shaped.layout.break_all_lines(max_width);
            let block = block_of(shaped, max_width);
            shaped.breaks.push(block);
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

    /// A parley editor that lays a text field's text out in `style`, as `block` lays out every
    /// other text: at a scale of 1 and with no rounding to whole pixels.
    pub(crate) fn editor(&self, style: TypeStyle) -> PlainEditor<u16> {
        let mut editor = PlainEditor::new(style.size);
        editor.set_quantize(false);
        let family = FontFamilyName::Named(self.families[style.font as usize].clone().into());
        let styles = editor.edit_styles();
        styles.insert(StyleProperty::Brush(0));
        styles.insert(StyleProperty::FontFamily(FontFamily::Single(family)));
        styles.insert(StyleProperty::FontWeight(FontWeight::new(f32::from(
            style.weight,
        ))));
        styles.insert(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        )));
        editor
    }

    /// The font and layout contexts an editor from `editor` lays its text out with.
    pub(crate) fn contexts(&mut self) -> (&mut FontContext, &mut LayoutContext<u16>) {
        (&mut self.fonts, &mut self.layouts)
    }

    /// Shapes `text` with its spans in one paragraph. Each mana symbol's `{...}` is replaced by
    /// the object character parley lays an inline box out at, as wide as the em of the style
    /// in force there, and the symbol's glyph is shaped apart in the Mana font.
    fn shape(&mut self, text: &str, style: TypeStyle, spans: &[Span]) -> Shaped {
        self.runs += 1;
        let tokens = symbols::tokens(text);
        // Where each symbol's object character lands in `words`.
        let mut objects = Vec::with_capacity(tokens.len());
        let words = words_of(text, &tokens, &mut objects);
        // A byte offset of `text` as one of `words`: one inside a symbol goes to the symbol's
        // start, or its end for the end of a range.
        let moved = |offset: usize, end: bool| {
            let mut shift = 0;
            for (token, &object) in tokens.iter().zip(&objects) {
                if offset >= token.end {
                    shift += token.len() - symbols::OBJECT.len_utf8();
                } else if offset > token.start {
                    return object + if end { symbols::OBJECT.len_utf8() } else { 0 };
                } else {
                    break;
                }
            }
            offset - shift
        };
        // The style in force at a byte of `text`: the last span over it, or the text's own.
        let style_at = |offset: usize| {
            (spans.iter().rev())
                .find(|span| (span.range.start as usize..span.range.end as usize).contains(&offset))
                .map_or(style, |span| span.style)
        };
        let mut runs = Vec::with_capacity(spans.len() + tokens.len());
        runs.extend(spans.iter().map(|span| Run {
            style: span.style,
            color: span.color,
        }));
        let mut shaped_symbols = Vec::with_capacity(tokens.len());
        let mut boxes = Vec::with_capacity(tokens.len());
        for (index, (token, &object)) in tokens.iter().zip(&objects).enumerate() {
            let symbol = &text[token.start + 1..token.end - 1];
            let around = style_at(token.start);
            let em = around.size;
            let run = u16::try_from(runs.len() + 1).expect("a text has fewer than 65,536 runs");
            let (shaped, run) = self.shape_symbol(symbol, around, run);
            runs.push(run);
            shaped_symbols.push(shaped);
            boxes.push(InlineBox {
                id: index as u64,
                kind: InlineBoxKind::InFlow,
                index: object,
                width: em,
                height: em,
                baseline: Some(em * (1.0 - symbols::DROP)),
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        let ranges: Vec<(Range<usize>, TypeStyle)> = (spans.iter())
            .map(|span| {
                let range =
                    moved(span.range.start as usize, false)..moved(span.range.end as usize, true);
                (range, span.style)
            })
            .collect();
        let mut builder = self.builder(&words, style, &ranges);
        for inline in boxes {
            builder.push_inline_box(inline);
        }
        Shaped {
            layout: builder.build(&*words),
            base: Run { style, color: None },
            runs,
            symbols: shaped_symbols,
            breaks: Vec::new(),
            used: false,
        }
    }

    /// The disc and glyph of the mana symbol `symbol` (the text between its braces) in a line
    /// of `around`, whose glyph is run `run`, with that run's look.
    fn shape_symbol(&mut self, symbol: &str, around: TypeStyle, run: u16) -> (ShapedSymbol, Run) {
        let em = around.size;
        let (color, ink) = symbols::colors(symbol);
        let glyph_style = TypeStyle {
            font: Font::Mana,
            weight: 400,
            size: em * symbols::GLYPH_SIZE,
            line_height: 1.0 / symbols::GLYPH_SIZE,
        };
        // A symbol the Mana font has no glyph for shows its own letters in the font around it,
        // as `ManaSymbol` does.
        let (letters, glyph_style) = match symbols::code(symbol) {
            Some(code) => (code.to_string(), glyph_style),
            None => (
                symbol.to_owned(),
                TypeStyle {
                    font: around.font,
                    ..glyph_style
                },
            ),
        };
        let mut layout = self.builder_layout(&letters, glyph_style, &[]);
        layout.break_all_lines(None);
        let left = (em - layout.width()) / 2.0;
        let top = (em - layout.height()) / 2.0;
        let mut glyphs = Vec::with_capacity(1);
        for_each_glyph(&layout, |_, glyph, x, y| {
            glyphs.push(PlacedGlyph {
                glyph,
                run,
                x: left + x,
                y: top + y,
            });
        });
        (
            ShapedSymbol { color, em, glyphs },
            Run {
                style: glyph_style,
                color: Some(ink),
            },
        )
    }

    /// A layout of `text` in `style`, with `ranges` styled apart, and nothing more.
    fn builder_layout(
        &mut self,
        text: &str,
        style: TypeStyle,
        ranges: &[(Range<usize>, TypeStyle)],
    ) -> Layout<u16> {
        self.builder(text, style, ranges).build(text)
    }

    /// A builder for `text` in `style`, with each of `ranges` in its own style. The brush of a
    /// glyph is its run's index: 0 for `style`, and one past a range's index for its range.
    fn builder<'a>(
        &'a mut self,
        text: &'a str,
        style: TypeStyle,
        ranges: &[(Range<usize>, TypeStyle)],
    ) -> parley::RangedBuilder<'a, u16> {
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        let family =
            |font: Font| FontFamily::Single(FontFamilyName::named(&self.families[font as usize]));
        builder.push_default(StyleProperty::Brush(0));
        builder.push_default(StyleProperty::FontFamily(family(style.font)));
        builder.push_default(StyleProperty::FontSize(style.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
            style.weight,
        ))));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        )));
        for (index, (range, span)) in ranges.iter().enumerate() {
            let brush = u16::try_from(index + 1).expect("a text has fewer than 65,536 spans");
            builder.push(StyleProperty::Brush(brush), range.clone());
            builder.push(StyleProperty::FontFamily(family(span.font)), range.clone());
            builder.push(StyleProperty::FontSize(span.size), range.clone());
            builder.push(
                StyleProperty::FontWeight(FontWeight::new(f32::from(span.weight))),
                range.clone(),
            );
            builder.push(
                StyleProperty::LineHeight(LineHeight::FontSizeRelative(span.line_height)),
                range.clone(),
            );
        }
        builder
    }
}

/// Calls `each` with the brush, glyph and pen position of every glyph of `layout`, which has
/// been broken into lines.
fn for_each_glyph(layout: &Layout<u16>, mut each: impl FnMut(u16, u16, f32, f32)) {
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                let brush = run.style().brush;
                for glyph in run.positioned_glyphs() {
                    let id = u16::try_from(glyph.id).expect("an OpenType glyph id fits 16 bits");
                    each(brush, id, glyph.x, glyph.y);
                }
            }
        }
    }
}

/// The lines and glyphs of `layout`, already broken at `max_width`, every glyph in the look
/// `base`: a text field's text as its editor laid it out, with no spans or mana symbols.
pub(crate) fn plain_block(layout: &Layout<u16>, base: Run, max_width: Option<f32>) -> Block {
    let mut block = Block {
        max_width,
        width: 0.0,
        height: layout.height(),
        lines: Vec::new(),
        base,
        runs: Vec::new(),
        glyphs: Vec::new(),
        symbols: Vec::new(),
    };
    for line in layout.lines() {
        add_line(&mut block, &line);
    }
    for_each_glyph(layout, |run, glyph, x, y| {
        block.glyphs.push(PlacedGlyph { glyph, run, x, y });
    });
    block
}

/// Adds `line`'s box to `block`, widening the block to it.
fn add_line(block: &mut Block, line: &parley::Line<'_, u16>) {
    let metrics = line.metrics();
    let width = metrics.advance - metrics.hanging_advance;
    block.width = block.width.max(width);
    let range = line.text_range();
    let offset = |at: usize| u32::try_from(at).expect("a text is under 4 GB");
    block.lines.push(Line {
        rect: Rect {
            x: metrics.offset,
            y: metrics.block_min_coord,
            width,
            height: metrics.block_max_coord - metrics.block_min_coord,
        },
        start: offset(range.start),
        end: offset(range.end),
        baseline: metrics.baseline,
    });
}

/// The lines, glyphs and symbols of a shaped text already broken at `max_width`.
fn block_of(shaped: &Shaped, max_width: Option<f32>) -> Block {
    let layout = &shaped.layout;
    let mut block = Block {
        max_width,
        width: 0.0,
        height: layout.height(),
        lines: Vec::new(),
        base: shaped.base,
        runs: shaped.runs.clone(),
        glyphs: Vec::new(),
        symbols: Vec::new(),
    };
    for line in layout.lines() {
        add_line(&mut block, &line);
        for item in line.items() {
            let PositionedLayoutItem::InlineBox(inline) = item else {
                continue;
            };
            let Some(symbol) = usize::try_from(inline.id)
                .ok()
                .and_then(|id| shaped.symbols.get(id))
            else {
                continue;
            };
            block.symbols.push(Symbol {
                rect: Rect {
                    x: inline.x,
                    y: inline.y,
                    width: symbol.em,
                    height: symbol.em,
                },
                color: symbol.color,
            });
            block
                .glyphs
                .extend(symbol.glyphs.iter().map(|glyph| PlacedGlyph {
                    x: inline.x + glyph.x,
                    y: inline.y + glyph.y,
                    ..*glyph
                }));
        }
    }
    for_each_glyph(layout, |run, glyph, x, y| {
        block.glyphs.push(PlacedGlyph { glyph, run, x, y });
    });
    block
}

/// A bundled font's bytes as the shared buffer fontique reads.
fn file_data(data: &'static [u8]) -> std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> {
    std::sync::Arc::new(data)
}
