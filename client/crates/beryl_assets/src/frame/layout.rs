//! Where each part of Beryl's drawn card frame goes, and its text set into lines.
//!
//! The geometry follows the web interface's frame (`ui/src/design/components/card-frame.css`),
//! which sizes everything in em: a twelfth of the card's width for the full frame
//! (`card-full`), a tenth for the battlefield frame (`card-field`). Every length here is that em
//! times the CSS's number, so a face drawn at either texture size has the web frame's
//! proportions.
//!
//! Text is shaped and broken into lines with parley over the bundled fonts alone, as the client
//! core lays out its text (`kit_text::text`). A run of rules text may hold mana symbols in
//! `{...}` notation, which are set in the Mana font on a colored disc.

use std::ops::Range;

use kit_base::atlas::FaceSize;
use kit_text::text::fonts::{ALL, Font, register_all};
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, FontWeight, Layout,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
};

use crate::encode::dimensions;

/// A rectangle in texture pixels, from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Box2 {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl Box2 {
    pub(crate) fn right(self) -> f32 {
        self.x + self.width
    }

    pub(crate) fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// The box with `top`, `right`, `bottom` and `left` taken off its sides (negative values
    /// add to it).
    pub(crate) fn inset(self, top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            x: self.x + left,
            y: self.y + top,
            width: self.width - left - right,
            height: self.height - top - bottom,
        }
    }
}

/// The two frames: the full frame, read whole, and the battlefield frame, with a thinner top,
/// a slimmer name bar and no text boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Variant {
    Full,
    Field,
}

impl Variant {
    /// The full frame for full-size faces, the battlefield frame for board-size ones.
    pub(crate) fn of(size: FaceSize) -> Self {
        match size {
            FaceSize::Full => Self::Full,
            FaceSize::Board => Self::Field,
        }
    }
}

/// Where each part of a frame lies on a face `width` by `height` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) variant: Variant,
    /// The card's em: a twelfth of its width on the full frame, a tenth on the battlefield's.
    pub(crate) em: f32,
    /// The whole face, and the black (or a token's pewter) body's corner radius.
    pub(crate) card: Box2,
    pub(crate) card_radius: f32,
    /// The colored rim inset in the body.
    pub(crate) rim: Box2,
    pub(crate) name_bar: Box2,
    pub(crate) name_padding: f32,
    pub(crate) name_size: f32,
    pub(crate) art: Box2,
    /// The full frame's type bar and rules box.
    pub(crate) type_bar: Option<Box2>,
    pub(crate) rules: Option<Box2>,
    /// The em of the power and toughness plaque and of the loyalty shield, which the full frame
    /// draws smaller.
    pub(crate) plaque_em: f32,
}

impl Geometry {
    /// The frame for a face at `size`.
    pub(crate) fn new(size: FaceSize) -> Self {
        let (width, height) = dimensions(size);
        let (width, height) = (width as f32, height as f32);
        let variant = Variant::of(size);
        let em = match variant {
            Variant::Full => width / 12.0,
            Variant::Field => width / 10.0,
        };
        let card = Box2 {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        // `.card` and `.card-field`'s padding, then `.card-rim`'s and `.card-field .card-rim`'s.
        let (body, rim_padding) = match variant {
            Variant::Full => ([0.48, 0.48, 0.84, 0.48], [0.3, 0.44, 0.23, 0.44]),
            Variant::Field => ([0.3, 0.56, 0.95, 0.56], [0.24, 0.36, 0.1, 0.36]),
        };
        let rim = card.inset(body[0] * em, body[1] * em, body[2] * em, body[3] * em);
        let content = rim.inset(
            rim_padding[0] * em,
            rim_padding[1] * em,
            rim_padding[2] * em,
            rim_padding[3] * em,
        );
        let gap = 0.2 * em;
        // The name bar spans the rim's padding: `margin: 0 -0.38em` (`-0.3em` on the field).
        let (bar_height, bar_margin, name_padding, name_size) = match variant {
            Variant::Full => (1.12, 0.38, 0.4, 0.92),
            Variant::Field => (1.0, 0.3, 0.26, 0.86),
        };
        let name_bar = Box2 {
            x: content.x - bar_margin * em,
            y: content.y,
            width: content.width + 2.0 * bar_margin * em,
            height: bar_height * em,
        };
        let below_name = name_bar.bottom() + gap;
        let (art, type_bar, rules) = match variant {
            Variant::Field => {
                let art = Box2 {
                    x: content.x,
                    y: below_name,
                    width: content.width,
                    height: content.bottom() - below_name,
                };
                (art, None, None)
            }
            Variant::Full => {
                let art = Box2 {
                    x: content.x,
                    y: below_name,
                    width: content.width,
                    height: 7.3 * em,
                };
                // The type bar's sizes are in its own em, 0.8 of the card's.
                let type_em = 0.8 * em;
                let type_bar = Box2 {
                    x: content.x - 0.5 * type_em,
                    y: art.bottom() + gap,
                    width: content.width + type_em,
                    height: 1.32 * type_em,
                };
                let top = type_bar.bottom() + gap;
                let rules = Box2 {
                    x: content.x,
                    y: top,
                    width: content.width,
                    height: content.bottom() - top,
                };
                (art, Some(type_bar), Some(rules))
            }
        };
        Self {
            variant,
            em,
            card,
            card_radius: 0.6 * em,
            rim,
            name_bar,
            name_padding: name_padding * em,
            name_size: name_size * em,
            art,
            type_bar,
            rules,
            plaque_em: match variant {
                Variant::Full => 0.8 * em,
                Variant::Field => em,
            },
        }
    }
}

/// How a text is set: its font, weight, size in pixels, line height as a multiple of its size,
/// and color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextStyle {
    pub(crate) font: Font,
    pub(crate) weight: f32,
    pub(crate) size: f32,
    pub(crate) line_height: f32,
    pub(crate) color: [u8; 3],
}

/// How a run of glyphs is drawn, carried through parley as the text's brush.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Ink {
    pub(crate) color: [u8; 3],
    /// The variable font's weight, which the rasterizer sets on its weight axis.
    pub(crate) weight: f32,
    /// For a mana symbol, the disc drawn behind it, and whether a black ring edges the disc,
    /// as in a cost.
    pub(crate) disc: Option<[u8; 3]>,
    pub(crate) ringed: bool,
}

/// One glyph set in a line: which glyph of which font, its pen position on the baseline, and
/// how it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SetGlyph {
    pub(crate) font: Font,
    pub(crate) glyph: u16,
    pub(crate) size: f32,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) ink: Ink,
}

/// A text set into lines from a top left corner at the origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SetText {
    /// The widest line's width and all the lines' height.
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) glyphs: Vec<SetGlyph>,
}

/// The mana symbols' code points in the Mana font, which `ui/src/components/Mana.tsx` uses too.
fn mana_code(symbol: &str) -> Option<char> {
    let code = match symbol {
        "W" => 0xe600,
        "U" => 0xe601,
        "B" => 0xe602,
        "R" => 0xe603,
        "G" => 0xe604,
        "C" => 0xe904,
        "X" => 0xe615,
        "S" => 0xe619,
        "T" => 0xe61a,
        "Q" => 0xe61b,
        "E" => 0xe907,
        number => 0xe605 + number.parse::<u32>().ok().filter(|n| *n < 16)?,
    };
    char::from_u32(code)
}

/// The loyalty symbol's code point in the Mana font.
pub(crate) const LOYALTY_SHIELD: char = '\u{e628}';

/// A mana symbol's disc, as `mana-symbols.css` colors it: the color's own, or the generic
/// stone for numbers and the other symbols.
fn disc_color(symbol: &str) -> [u8; 3] {
    match symbol {
        "W" => [0xf6, 0xf2, 0xc8],
        "U" => [0xc4, 0xec, 0xfb],
        "B" => [0xb3, 0xa8, 0xa3],
        "R" => [0xef, 0x9c, 0x7c],
        "G" => [0x9f, 0xcc, 0xa5],
        _ => [0xd3, 0xcb, 0xc1],
    }
}

/// The mana symbol's glyph color.
const SYMBOL_INK: [u8; 3] = [0x16, 0x14, 0x12];

/// Each mana symbol of a text: its byte range and its disc's color.
type Symbols = Vec<(Range<usize>, [u8; 3])>;

/// A text with its `{...}` symbols replaced by the Mana font's code points, and the byte range
/// and disc color of each. A symbol the font has no glyph for stays as written.
fn with_symbols(text: &str) -> (String, Symbols) {
    let mut out = String::with_capacity(text.len());
    let mut symbols = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open..];
        let Some(close) = after.find('}') else {
            out.push_str(after);
            return (out, symbols);
        };
        let symbol = &after[1..close];
        match mana_code(symbol) {
            Some(code) => {
                let start = out.len();
                out.push(code);
                symbols.push((start..out.len(), disc_color(symbol)));
            }
            None => out.push_str(&after[..=close]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    (out, symbols)
}

/// A symbol's glyph as a share of the text it sits in: `.mana-symbol`'s `0.82em`.
pub(crate) const SYMBOL_SCALE: f32 = 0.82;
/// A symbol's disc as a multiple of its glyph's size: `.mana-symbol` is `1.25em` wide in an
/// element whose font is `0.82em`.
pub(crate) const DISC_SCALE: f32 = 1.25 / SYMBOL_SCALE;

/// Shapes and sets text in the bundled fonts.
pub(crate) struct Typesetter {
    fonts: FontContext,
    layouts: LayoutContext<Ink>,
    /// Each bundled font's family name, as parley knows it, in the order of [`Font`].
    families: Vec<String>,
}

impl Typesetter {
    /// A font context holding only the bundled fonts, with system fonts switched off.
    pub(crate) fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let families = register_all(|file| {
            let data: std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> =
                std::sync::Arc::new(file.data);
            let registered = collection.register_fonts(Blob::new(data), None);
            let (family, _) = registered
                .first()
                .unwrap_or_else(|| panic!("{} holds a font", file.file));
            collection
                .family_name(*family)
                .expect("a registered family has a name")
                .to_owned()
        });
        Self {
            fonts: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layouts: LayoutContext::new(),
            families,
        }
    }

    /// `text` in `style`, broken into lines at `max_width` (on one line with `None`), with its
    /// `{...}` mana symbols drawn on discs, `ringed` as in a cost.
    pub(crate) fn set(
        &mut self,
        text: &str,
        style: TextStyle,
        max_width: Option<f32>,
        ringed: bool,
    ) -> SetText {
        self.set_styled(text, style, max_width, ringed, Alignment::Start, 0.0)
    }

    /// Sets the capitals on an ability plate's tab, with its CSS letter spacing.
    pub(crate) fn set_tab(&mut self, text: &str, style: TextStyle, spacing: f32) -> SetText {
        self.set_styled(text, style, None, false, Alignment::Start, spacing)
    }

    fn set_styled(
        &mut self,
        text: &str,
        style: TextStyle,
        max_width: Option<f32>,
        ringed: bool,
        alignment: Alignment,
        spacing: f32,
    ) -> SetText {
        let (text, symbols) = with_symbols(text);
        let families = self.families.clone();
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, &text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(family(style.font, &families)));
        builder.push_default(StyleProperty::FontSize(style.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
        builder.push_default(StyleProperty::LetterSpacing(spacing));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        )));
        builder.push_default(StyleProperty::Brush(Ink {
            color: style.color,
            weight: style.weight,
            disc: None,
            ringed: false,
        }));
        let symbol_size = style.size * SYMBOL_SCALE;
        for (range, disc) in &symbols {
            builder.push(
                StyleProperty::FontFamily(family(Font::Symbols, &families)),
                range.clone(),
            );
            builder.push(StyleProperty::FontSize(symbol_size), range.clone());
            // Room on both sides for the disc, which is wider than the glyph's advance.
            builder.push(
                StyleProperty::LetterSpacing(symbol_size * (DISC_SCALE - 1.0)),
                range.clone(),
            );
            builder.push(
                StyleProperty::Brush(Ink {
                    color: SYMBOL_INK,
                    weight: 400.0,
                    disc: Some(*disc),
                    ringed,
                }),
                range.clone(),
            );
        }
        let mut layout: Layout<Ink> = builder.build(&text);
        layout.set_alignment(alignment, AlignmentOptions::default());
        layout.break_all_lines(max_width);
        set_text(&layout, symbol_size * (DISC_SCALE - 1.0) / 2.0)
    }

    /// `text` set on one line in `style`, shrunk to be no wider than `room`, as `fit.ts` shrinks
    /// a long name or type line.
    pub(crate) fn set_line_to_fit(&mut self, text: &str, style: TextStyle, room: f32) -> SetText {
        let set = self.set(text, style, None, true);
        if set.width <= room || set.width <= 0.0 {
            return set;
        }
        let scale = room / set.width;
        self.set(
            text,
            TextStyle {
                size: style.size * scale,
                ..style
            },
            None,
            true,
        )
    }

    /// `text` broken at `room.0` and shrunk until it is no taller than `room.1`, as the web
    /// interface's `useRulesFit` shrinks rules text: text rewraps as it shrinks, so its height
    /// falls faster than its size, and each step shrinks by the square root of the overflow.
    pub(crate) fn set_block_to_fit(
        &mut self,
        text: &str,
        style: TextStyle,
        room: (f32, f32),
        centered: bool,
    ) -> SetText {
        let (width, height) = room;
        let mut scale = 1.0;
        let alignment = if centered {
            Alignment::Center
        } else {
            Alignment::Start
        };
        let mut set = self.set_styled(text, style, Some(width), false, alignment, 0.0);
        for _ in 0..8 {
            if height <= 0.0 || set.height <= height + 0.5 {
                break;
            }
            scale *= (height / set.height).sqrt() * 0.98;
            set = self.set_styled(
                text,
                TextStyle {
                    size: style.size * scale,
                    ..style
                },
                Some(width),
                false,
                alignment,
                0.0,
            );
        }
        set
    }
}

/// The family parley knows `font` by, from the families in the order of [`Font`].
fn family(font: Font, families: &[String]) -> FontFamily<'_> {
    FontFamily::Single(FontFamilyName::named(&families[font as usize]))
}

/// Which bundled font `data` is, by the address of its bytes: parley hands back the blob each
/// font was registered with.
fn font_of(data: &[u8]) -> Font {
    ALL.iter()
        .find(|file| std::ptr::eq(file.data.as_ptr(), data.as_ptr()))
        .map_or(Font::CrimsonPro, |file| file.font)
}

/// The glyphs of a laid-out text. A mana symbol's glyph moves right by `symbol_shift`, half the
/// letter spacing that made room for its disc, so that it sits in the disc's middle.
fn set_text(layout: &Layout<Ink>, symbol_shift: f32) -> SetText {
    let mut set = SetText {
        height: layout.height(),
        ..SetText::default()
    };
    for line in layout.lines() {
        let metrics = line.metrics();
        set.width = set.width.max(metrics.advance - metrics.hanging_advance);
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let font = font_of(run.run().font().data.as_ref());
            let size = run.run().font_size();
            let ink = run.style().brush;
            let shift = if ink.disc.is_some() {
                symbol_shift
            } else {
                0.0
            };
            set.glyphs
                .extend(run.positioned_glyphs().map(|glyph| SetGlyph {
                    font,
                    glyph: u16::try_from(glyph.id).unwrap_or(0),
                    size,
                    x: glyph.x + shift,
                    y: glyph.y,
                    ink,
                }));
        }
    }
    set
}
