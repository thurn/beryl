//! Beryl's drawn card frame: the picture of a face whose scan has not been downloaded
//! (docs/beryl-3d.md, "Card faces"). It is the web interface's frame (`Card` in
//! `ui/src/components/Card.tsx`, with `card-frame.css` and `card-palettes.css`) drawn into the
//! face's texture: a black body (pewter for a token), a rim in the card's color, the name bar
//! with the mana cost, a stand-in painting for the art, and on the full frame the type bar and
//! the rules box, with the printed power and toughness or loyalty at the foot.
//!
//! A board-size face draws the battlefield frame (`card-field`), a full-size face the full frame
//! (`card-full`). A face that shows an ability on the stack is its source's picture with the
//! plate the web interface's `AbilityPlate` draws over the lower half, saying the ability; the
//! plate goes over a scan as well as over a drawn frame ([`plate_over`]).
//!
//! The frame's colors and lengths are the web stylesheets' own values, so that both clients draw
//! the same frame.
//!
//! Only the asset workers draw frames, never the frame thread: [`frames_drawn_on_this_thread`]
//! counts them.
//!
//! The card back has no frame: without its scan it is drawn as the web interface's plain back
//! ([`card_back`]).

mod back;
mod layout;
mod paint;

use std::cell::{Cell, RefCell};

use beryl_scene::faces::{Plate, PrintedText};
use beryl_scene::input::FaceSize;
use beryl_scene::text::fonts::Font;
use tiny_skia::{Pixmap, Point, Shader};

pub use back::card_back;

use crate::encode::dimensions;
use crate::faces::MatchFace;
use layout::{Box2, Geometry, LOYALTY_SHIELD, TextStyle, Typesetter, Variant};
use paint::{Canvas, Rgb, color, hsl, linear, mix, radial, stops, vertical};

thread_local! {
    static FRAMES_DRAWN: Cell<u64> = const { Cell::new(0) };
    static TYPESETTER: RefCell<Option<Typesetter>> = const { RefCell::new(None) };
}

/// How many frames and plates this thread has drawn. Only the asset workers draw them, so on
/// the thread that draws frames this stays zero.
#[must_use]
pub fn frames_drawn_on_this_thread() -> u64 {
    FRAMES_DRAWN.with(Cell::get)
}

/// One color's frame: the rim, its highlight and the dark line that edges it, and the bars.
#[derive(Clone, Copy)]
struct Palette {
    rim: Rgb,
    rim_hi: Rgb,
    rim_lo: Rgb,
    bar: Rgb,
    bar_hi: Rgb,
}

/// `card-palettes.css`'s `.frame-<key>` palettes.
fn palette(key: char) -> Palette {
    let p = |rim, rim_hi, rim_lo, bar, bar_hi| Palette {
        rim,
        rim_hi,
        rim_lo,
        bar,
        bar_hi,
    };
    match key {
        'w' => p(
            [0xd3, 0xd1, 0xcd],
            [0xf8, 0xf8, 0xf7],
            [0x2e, 0x2b, 0x30],
            [0xe2, 0xdf, 0xda],
            [0xf5, 0xf3, 0xf0],
        ),
        'u' => p(
            [0x2a, 0x7f, 0xc4],
            [0x63, 0xb3, 0xe8],
            [0x06, 0x27, 0x5a],
            [0xb9, 0xd3, 0xea],
            [0xdc, 0xf0, 0xfb],
        ),
        'b' => p(
            [0x2e, 0x2b, 0x29],
            [0x5a, 0x55, 0x51],
            [0x0c, 0x0b, 0x0a],
            [0xb4, 0xad, 0xa8],
            [0xd3, 0xcd, 0xc8],
        ),
        'r' => p(
            [0xcc, 0x2e, 0x14],
            [0xf7, 0x74, 0x3e],
            [0x6d, 0x08, 0x06],
            [0xe0, 0xb4, 0xa0],
            [0xf0, 0xd2, 0xc4],
        ),
        'g' => p(
            [0x2a, 0x7e, 0x51],
            [0x63, 0xae, 0x80],
            [0x0d, 0x2f, 0x19],
            [0xb6, 0xbc, 0xb0],
            [0xd6, 0xdb, 0xd0],
        ),
        'm' => p(
            [0xd9, 0xb4, 0x6a],
            [0xf3, 0xda, 0x9b],
            [0x6f, 0x54, 0x22],
            [0xda, 0xb8, 0x72],
            [0xec, 0xd1, 0x97],
        ),
        _ => p(
            [0xdd, 0xd4, 0xc2],
            [0xf7, 0xf2, 0xe8],
            [0x6e, 0x55, 0x36],
            [0xd0, 0xc7, 0xbc],
            [0xe2, 0xdb, 0xd1],
        ),
    }
}

/// A multicolored card's accent, the first color's: `.accent-<color>`.
fn accent(color: &str) -> Option<Rgb> {
    match color {
        "W" => Some([0x8a, 0x80, 0x70]),
        "U" => Some([0x0f, 0x5f, 0xa8]),
        "B" => Some([0x2a, 0x26, 0x24]),
        "R" => Some([0xa5, 0x20, 0x0c]),
        "G" => Some([0x1d, 0x6a, 0x40]),
        _ => None,
    }
}

/// The frame's palette key, as `frameKey` in `Card.tsx` gives it: a color's letter, `m` for
/// multicolored, `c` for colorless. A colorless land with one basic land type takes that type's
/// color.
fn frame_key(colors: &[String], type_line: &str) -> char {
    let basics = [
        ("Plains", 'w'),
        ("Island", 'u'),
        ("Swamp", 'b'),
        ("Mountain", 'r'),
        ("Forest", 'g'),
    ];
    match colors {
        [] if type_line.contains("Land") => {
            let mut types = basics.iter().filter(|(name, _)| type_line.contains(name));
            match (types.next(), types.next()) {
                (Some((_, key)), None) => *key,
                _ => 'c',
            }
        }
        [] => 'c',
        [one] => one.to_lowercase().chars().next().unwrap_or('c'),
        _ => 'm',
    }
}

/// The hue `Card.tsx`'s stand-in painting starts from, for each palette key.
fn hue(key: char) -> f32 {
    match key {
        'w' => 42.0,
        'u' => 205.0,
        'b' => 265.0,
        'r' => 14.0,
        'g' => 115.0,
        'm' => 40.0,
        _ => 28.0,
    }
}

/// The FNV-1a hash `Card.tsx` gives a name, over its UTF-16 code units, so that a card's stand-in
/// painting is the same in both clients.
fn hash(text: &str) -> u32 {
    text.encode_utf16().fold(2_166_136_261_u32, |h, unit| {
        (h ^ u32::from(unit)).wrapping_mul(16_777_619)
    })
}

/// The ink of names, type lines, rules text and the slash between power and toughness.
const INK: Rgb = [0x14, 0x12, 0x10];

/// The RGBA pixels at `size` of `face` drawn as Beryl's frame, with its plate when it shows an
/// ability on the stack.
#[must_use]
pub fn frame(face: &MatchFace, size: FaceSize) -> Vec<u8> {
    FRAMES_DRAWN.with(|drawn| drawn.set(drawn.get() + 1));
    with_typesetter(|text| {
        let (width, height) = dimensions(size);
        let pixmap = Pixmap::new(width, height).expect("a face has an area");
        let mut canvas = Canvas::new(pixmap);
        let name = face.names.first().map_or("", String::as_str);
        draw_frame(&mut canvas, text, name, &face.colors, &face.text, size);
        if let Some(plate) = &face.plate {
            draw_plate(&mut canvas, text, plate, size);
        }
        canvas.pixmap.take()
    })
}

/// `pixels`, a decoded scan at `size`, with `plate` drawn over its lower half.
#[must_use]
pub fn plate_over(pixels: Vec<u8>, plate: &Plate, size: FaceSize) -> Vec<u8> {
    FRAMES_DRAWN.with(|drawn| drawn.set(drawn.get() + 1));
    let (width, height) = dimensions(size);
    let Some(size_of) = tiny_skia::IntSize::from_wh(width, height) else {
        return pixels;
    };
    // A decoded scan is opaque, so its pixels are already premultiplied.
    let Some(pixmap) = Pixmap::from_vec(pixels, size_of) else {
        return Vec::new();
    };
    with_typesetter(|text| {
        let mut canvas = Canvas::new(pixmap);
        draw_plate(&mut canvas, text, plate, size);
        canvas.pixmap.take()
    })
}

/// Runs `draw` with this thread's typesetter, made the first time: registering the bundled
/// fonts is the slow part.
fn with_typesetter<T>(draw: impl FnOnce(&mut Typesetter) -> T) -> T {
    TYPESETTER.with(|cell| {
        let mut cell = cell.borrow_mut();
        draw(cell.get_or_insert_with(Typesetter::new))
    })
}

fn draw_frame(
    canvas: &mut Canvas,
    text: &mut Typesetter,
    name: &str,
    colors: &[String],
    printed: &PrintedText,
    size: FaceSize,
) {
    let g = Geometry::new(size);
    let em = g.em;
    let key = frame_key(colors, &printed.type_line);
    let palette = palette(key);
    let accent = (colors.len() > 1)
        .then(|| colors.first().and_then(|c| accent(c)))
        .flatten();
    // The corners outside the card's rounded body, as behind a scan's.
    canvas.pixmap.fill(color([0x0b, 0x0a, 0x09], 1.0));
    // The body: black, deepest at the bottom, or a token's pewter.
    let body = if printed.token {
        [[0xb4, 0xb0, 0xaa], [0x74, 0x70, 0x6b]]
    } else {
        [[0x25, 0x23, 0x21], [0x1a, 0x19, 0x18]]
    };
    let shade = vertical(g.card, stops(&[(0.0, body[0], 1.0), (1.0, body[1], 1.0)]));
    canvas.fill_box(g.card, g.card_radius, shade);
    canvas.edge_inside(
        g.card,
        g.card_radius,
        0.06 * em,
        color([0x2e, 0x2b, 0x29], 1.0),
    );
    draw_rim(canvas, &g, palette);
    draw_bar(
        canvas,
        g.name_bar,
        palette,
        accent,
        (if accent.is_some() { 0.14 } else { 0.07 }) * em,
    );
    draw_name(canvas, text, &g, name, printed);
    draw_art(canvas, &g, name, key, palette, accent);
    let has_stats = printed.power.is_some() && printed.toughness.is_some();
    if let (Some(type_bar), Some(rules)) = (g.type_bar, g.rules) {
        draw_bar(canvas, type_bar, palette, accent, 0.12 * 0.8 * em);
        let type_style = TextStyle {
            font: Font::CrimsonPro,
            weight: 650.0,
            size: 0.8 * em,
            line_height: 1.0,
            color: INK,
        };
        let padding = 0.5 * 0.8 * em;
        let set = text.set_line_to_fit(
            &printed.type_line,
            type_style,
            type_bar.width - 2.0 * padding,
        );
        canvas.text(
            &set,
            (
                type_bar.x + padding,
                type_bar.y + (type_bar.height - set.height) / 2.0,
            ),
        );
        draw_rules(
            canvas,
            text,
            rules,
            palette,
            printed,
            has_stats || printed.loyalty.is_some(),
            em,
        );
    }
    if let (Some(power), Some(toughness)) = (&printed.power, &printed.toughness) {
        draw_stats(canvas, text, &g, palette, power, toughness);
    } else if let Some(loyalty) = &printed.loyalty {
        draw_loyalty(canvas, text, &g, loyalty);
    }
}

/// The name in the name bar, shrunk to fit beside the mana cost, which the full frame prints at
/// the bar's right end.
fn draw_name(
    canvas: &mut Canvas,
    text: &mut Typesetter,
    g: &Geometry,
    name: &str,
    printed: &PrintedText,
) {
    let em = g.em;
    let cost = match g.variant {
        Variant::Full => printed.mana_cost.as_deref().filter(|c| !c.is_empty()),
        Variant::Field => None,
    };
    let cost = cost.map(|cost| {
        // `.mana-cost .mana-symbol`'s font is 0.66em, so its glyph is 0.82 of that.
        let style = TextStyle {
            font: Font::Mana,
            weight: 400.0,
            size: 0.66 * em,
            line_height: 1.0,
            color: INK,
        };
        text.set(cost, style, None, true)
    });
    let cost_width = cost.as_ref().map_or(0.0, |c| c.width + 0.3 * em);
    let name_room = g.name_bar.width - 2.0 * g.name_padding - cost_width;
    let name_style = TextStyle {
        font: Font::CrimsonPro,
        weight: 750.0,
        size: g.name_size,
        line_height: 1.0,
        color: INK,
    };
    let set = text.set_line_to_fit(name, name_style, name_room);
    let bar = g.name_bar;
    canvas.text(
        &set,
        (
            bar.x + g.name_padding,
            bar.y + (bar.height - set.height) / 2.0,
        ),
    );
    if let Some(cost) = cost {
        let x = bar.right() - g.name_padding - cost.width;
        canvas.text(&cost, (x, bar.y + (bar.height - cost.height) / 2.0));
    }
}

/// The colored rim inset in the body: its color with a highlight a little inside each side,
/// edged by its dark line, and on the full frame faint diagonal streaks.
fn draw_rim(canvas: &mut Canvas, g: &Geometry, palette: Palette) {
    let rim = g.rim;
    let em = g.em;
    let radius = 0.3 * em;
    let at = |length: f32| (length / rim.width).clamp(0.0, 1.0);
    let across = linear(
        Point::from_xy(rim.x, rim.y),
        Point::from_xy(rim.right(), rim.y),
        stops(&[
            (at(0.08 * em), palette.rim, 1.0),
            (at(0.16 * em), palette.rim_hi, 1.0),
            (at(0.28 * em), palette.rim, 1.0),
            (1.0 - at(0.28 * em), palette.rim, 1.0),
            (1.0 - at(0.16 * em), palette.rim_hi, 1.0),
            (1.0 - at(0.08 * em), palette.rim, 1.0),
        ]),
    );
    canvas.fill_box(rim, radius, across);
    if g.variant == Variant::Full {
        // `--rim-streaks`: light bands at 172 degrees, 1.3em apart.
        let period = 1.3 * em;
        let (dx, dy) = (8.0_f32.to_radians().sin(), 8.0_f32.to_radians().cos());
        let band = |offset: f32| -> (f32, Rgb, f32) { (offset, [0xff; 3], 0.0) };
        let streaks = tiny_skia::LinearGradient::new(
            Point::from_xy(rim.x, rim.y),
            Point::from_xy(rim.x + dx * period, rim.y + dy * period),
            stops(&[
                band(0.0),
                band(0.5 / 1.3),
                (0.65 / 1.3, [0xff; 3], 0.16),
                band(0.8 / 1.3),
                band(1.0),
            ]),
            tiny_skia::SpreadMode::Repeat,
            tiny_skia::Transform::identity(),
        )
        .unwrap_or(Shader::SolidColor(tiny_skia::Color::TRANSPARENT));
        canvas.fill_box(rim, radius, streaks);
    }
    canvas.edge_inside(rim, radius, 0.05 * em, color(palette.rim_lo, 1.0));
}

/// A name or type bar: the bars' color, lighter at the top and darker at the foot, in a pill
/// edged by the rim's dark line (or, for a multicolored card, by its accent over the left part).
fn draw_bar(canvas: &mut Canvas, bar: Box2, palette: Palette, accent: Option<Rgb>, edge: f32) {
    let radius = bar.height / 2.0;
    let border = match accent {
        Some(accent) => linear(
            Point::from_xy(bar.x, bar.y),
            Point::from_xy(bar.right(), bar.y),
            stops(&[(0.45, accent, 1.0), (0.55, palette.rim_lo, 1.0)]),
        ),
        None => Shader::SolidColor(color(palette.rim_lo, 1.0)),
    };
    canvas.fill_box(bar, radius, border);
    let inner = bar.inset(edge, edge, edge, edge);
    let dark = mix(palette.bar, 0.85, [0, 0, 0]);
    let fill = vertical(
        inner,
        stops(&[
            (0.0, palette.bar_hi, 1.0),
            (0.4, palette.bar, 1.0),
            (0.8, palette.bar, 1.0),
            (1.0, dark, 1.0),
        ]),
    );
    canvas.fill_box(inner, radius - edge, fill);
}

/// A deterministic stand-in painting in the card's colors, as `placeholderArt` in `Card.tsx`
/// paints it: a sky, a dark ridge, a shadowed hollow and a light source.
fn draw_art(
    canvas: &mut Canvas,
    g: &Geometry,
    name: &str,
    key: char,
    palette: Palette,
    accent: Option<Rgb>,
) {
    let art = g.art;
    let em = g.em;
    if let Some(accent) = accent {
        // A line of the accent along the art's left edge and its bottom edge.
        let line = Box2 {
            x: art.x - 0.23 * em,
            y: art.y,
            width: art.width + 0.23 * em,
            height: art.height + 0.2 * em,
        };
        canvas.fill_box(line, 0.0, Shader::SolidColor(color(palette.rim_lo, 1.0)));
        let line = line.inset(0.0, 0.0, 0.0, 0.05 * em);
        canvas.fill_box(line, 0.0, Shader::SolidColor(color(accent, 1.0)));
    }
    let h = hash(name);
    let hue = hue(key) + (h % 40) as f32 - 20.0;
    // JavaScript's `>>` shifts a signed 32-bit integer and its `%` keeps the dividend's sign,
    // so in `Card.tsx` a hash at or above 2^31 lifts the sun and the ridge above their least
    // heights. Rust's `i32` shifts and remainders do the same.
    #[expect(
        clippy::cast_possible_wrap,
        reason = "the hash is read as JavaScript's `>>` reads it, as a signed 32-bit integer"
    )]
    let signed = h as i32;
    let sun = (
        (20 + h % 60) as f32 / 100.0,
        (18 + (signed >> 8) % 25) as f32 / 100.0,
    );
    let ridge = (52 + (signed >> 16) % 18) as f32 / 100.0;
    let sky = vertical(
        art,
        stops(&[
            (0.0, hsl(hue + 10.0, 55.0, 72.0), 1.0),
            (0.55, hsl(hue, 60.0, 46.0), 1.0),
            (1.0, hsl(hue - 15.0, 55.0, 30.0), 1.0),
        ]),
    );
    canvas.fill_box(art, 0.0, sky);
    // The gradient runs toward 172 degrees, which CSS measures clockwise from straight up:
    // down and a little to the right, so the ridge's edge rises toward the right.
    let (dx, dy) = (8.0_f32.to_radians().sin(), 8.0_f32.to_radians().cos());
    let middle = (art.x + art.width / 2.0, art.y + art.height / 2.0);
    let half = f32::midpoint(art.width * dx.abs(), art.height * dy.abs());
    let ground = linear(
        Point::from_xy(middle.0 - dx * half, middle.1 - dy * half),
        Point::from_xy(middle.0 + dx * half, middle.1 + dy * half),
        stops(&[
            (ridge, hsl(hue - 10.0, 40.0, 24.0), 0.0),
            ((ridge + 0.04).min(1.0), hsl(hue - 10.0, 40.0, 24.0), 1.0),
            (1.0, hsl(hue - 20.0, 45.0, 12.0), 1.0),
        ]),
    );
    canvas.fill_box(art, 0.0, ground);
    let hollow = radial(
        Point::from_xy(
            art.x + (1.0 - sun.0) * art.width,
            art.y + (ridge + 0.3) * art.height,
        ),
        (0.8 * art.width, 0.45 * art.height),
        stops(&[
            (0.0, hsl(hue, 45.0, 18.0), 0.95),
            (0.7, hsl(hue, 45.0, 18.0), 0.0),
        ]),
    );
    canvas.fill_box(art, 0.0, hollow);
    let light = radial(
        Point::from_xy(art.x + sun.0 * art.width, art.y + sun.1 * art.height),
        // `circle ... 28%`: 28% of the distance from the middle to the farthest corner, as CSS
        // sizes a circle given no size.
        {
            let across = art.width * sun.0.max(1.0 - sun.0);
            let down = art.height * sun.1.max(1.0 - sun.1);
            let far = across.hypot(down) * 0.28;
            (far, far)
        },
        stops(&[
            (0.0, hsl(hue + 30.0, 95.0, 85.0), 0.9),
            (1.0, hsl(hue + 30.0, 95.0, 85.0), 0.0),
        ]),
    );
    canvas.fill_box(art, 0.0, light);
    canvas.edge_outside(art, 0.0, 0.06 * em, color(palette.rim_lo, 1.0));
}

/// The full frame's rules box: the bars' color thinned toward parchment, with the rules text
/// in its middle, shrunk to fit. `foot` leaves room for the plaque at its foot.
fn draw_rules(
    canvas: &mut Canvas,
    text: &mut Typesetter,
    rules: Box2,
    palette: Palette,
    printed: &PrintedText,
    foot: bool,
    em: f32,
) {
    let paper = mix(palette.bar, 0.44, [0xf8, 0xf5, 0xec]);
    let fill = vertical(
        rules,
        stops(&[(0.0, mix(paper, 0.92, [0xff; 3]), 1.0), (1.0, paper, 1.0)]),
    );
    canvas.fill_box(rules, 0.0, fill);
    canvas.edge_outside(rules, 0.0, 0.08 * 0.7 * em, color(palette.rim_lo, 1.0));
    canvas.edge_inside(rules, 0.0, 0.06 * 0.7 * em, color(palette.rim_lo, 0.45));
    let Some(words) = printed.rules.as_deref().filter(|w| !w.is_empty()) else {
        return;
    };
    // `.card-rules`: 0.7em text, padded 0.35em by 0.45em in that em, 1.1em at the foot beside
    // the plaque.
    let size = 0.7 * em;
    let bottom = if foot { 1.1 } else { 0.35 };
    let room = rules.inset(0.35 * size, 0.45 * size, bottom * size, 0.45 * size);
    let style = TextStyle {
        font: Font::CrimsonPro,
        weight: 500.0,
        size,
        line_height: 1.2,
        color: INK,
    };
    let set = text.set_block_to_fit(words, style, (room.width, room.height));
    let top = room.y + ((room.height - set.height) / 2.0).max(0.0);
    canvas.text(&set, (room.x, top));
}

/// The power and toughness plaque at the foot: the bars' color in a dark-edged frame,
/// numerals in blue-black ink, the slash in black.
fn draw_stats(
    canvas: &mut Canvas,
    text: &mut Typesetter,
    g: &Geometry,
    palette: Palette,
    power: &str,
    toughness: &str,
) {
    let p = g.plaque_em;
    let numeral = TextStyle {
        font: Font::CrimsonPro,
        weight: 700.0,
        size: 1.7 * p,
        line_height: 1.0,
        color: [0x19, 0x21, 0x3a],
    };
    let slash = TextStyle {
        size: 0.8 * 1.7 * p,
        color: INK,
        ..numeral
    };
    let parts = [
        text.set(power, numeral, None, false),
        text.set("/", slash, None, false),
        text.set(toughness, numeral, None, false),
    ];
    let words: f32 = parts.iter().map(|part| part.width).sum();
    let width = (words + 0.7 * p).max(4.2 * p);
    let plaque = Box2 {
        x: g.card.right() - 0.68 * p - width,
        y: g.card.bottom() - 0.3 * p - 1.9 * p,
        width,
        height: 1.9 * p,
    };
    let radius = 0.5 * p;
    canvas.fill_box(
        plaque,
        radius,
        Shader::SolidColor(color([0x14, 0x11, 0x0f], 1.0)),
    );
    let inner = plaque.inset(0.12 * p, 0.12 * p, 0.12 * p, 0.12 * p);
    let ring = mix(palette.bar, 0.55, [0x4a, 0x3a, 0x28]);
    canvas.fill_box(
        inner,
        radius - 0.12 * p,
        Shader::SolidColor(color(ring, 1.0)),
    );
    let face = inner.inset(0.18 * p, 0.18 * p, 0.18 * p, 0.18 * p);
    let fill = vertical(
        face,
        stops(&[
            (0.0, palette.bar_hi, 1.0),
            (0.45, palette.bar, 1.0),
            (1.0, mix(palette.bar, 0.88, [0, 0, 0]), 1.0),
        ]),
    );
    canvas.fill_box(face, radius - 0.3 * p, fill);
    // The numerals share a baseline; the plaque's padding lifts them 0.18em off its middle.
    let tallest = parts[0].height.max(parts[2].height);
    let top = plaque.y + (plaque.height - 0.18 * p - tallest) / 2.0;
    let mut x = plaque.x + (plaque.width - words) / 2.0;
    for (index, part) in parts.iter().enumerate() {
        // The slash is set smaller; its baseline matches the numerals'.
        let y = if index == 1 {
            top + (tallest - part.height) * 0.8
        } else {
            top
        };
        canvas.text(part, (x, y));
        x += part.width;
    }
}

/// A planeswalker's loyalty on the printed loyalty shield, from the Mana font, pale with dark
/// numerals.
fn draw_loyalty(canvas: &mut Canvas, text: &mut Typesetter, g: &Geometry, loyalty: &str) {
    let l = g.plaque_em;
    let area = Box2 {
        x: g.card.right() - 0.5 * l - 4.0 * l,
        y: g.card.bottom() - 0.2 * l - 3.6 * l,
        width: 4.0 * l,
        height: 3.6 * l,
    };
    let shield = text.set(
        &LOYALTY_SHIELD.to_string(),
        TextStyle {
            font: Font::Mana,
            weight: 400.0,
            size: 3.7 * l,
            line_height: 1.0,
            color: [0xec, 0xe6, 0xda],
        },
        None,
        false,
    );
    canvas.text(
        &shield,
        (
            area.x + (area.width - shield.width) / 2.0,
            area.y + (area.height - shield.height) / 2.0,
        ),
    );
    let number = text.set(
        loyalty,
        TextStyle {
            font: Font::CrimsonPro,
            weight: 700.0,
            size: 1.8 * l,
            line_height: 1.0,
            color: [0x1c, 0x1a, 0x19],
        },
        None,
        false,
    );
    let x = area.x + (area.width - number.width) / 2.0;
    let y = area.y + (area.height - number.height) / 2.0 + 0.16 * 1.8 * l;
    canvas.text(&number, (x, y));
}

/// The plate over the lower half of a face that says one ability, as `AbilityPlate` in
/// `Card.tsx` draws it: parchment in a dark rim, its label on a tab across its top, and the
/// ability's words in its middle, shrunk to fit.
fn draw_plate(canvas: &mut Canvas, text: &mut Typesetter, plate: &Plate, size: FaceSize) {
    let g = Geometry::new(size);
    let em = g.em;
    let card = g.card;
    let area = Box2 {
        x: card.x + 0.05 * card.width,
        y: card.y + 0.56 * card.height,
        width: 0.9 * card.width,
        height: (1.0 - 0.56 - 0.045) * card.height,
    };
    let radius = 0.35 * em;
    canvas.fill_box(
        area,
        radius,
        Shader::SolidColor(color([0x1b, 0x17, 0x12], 1.0)),
    );
    let paper = area.inset(0.08 * em, 0.08 * em, 0.08 * em, 0.08 * em);
    let fill = vertical(
        paper,
        stops(&[
            (0.0, [0xf8, 0xf2, 0xe2], 1.0),
            (1.0, [0xe6, 0xdc, 0xc4], 1.0),
        ]),
    );
    canvas.fill_box(paper, radius - 0.08 * em, fill);
    canvas.edge_inside(
        paper,
        radius - 0.08 * em,
        0.1 * em,
        color([150, 120, 70], 0.45),
    );
    // The label: `.ability-plate-label`, 0.62em capitals on a dark tab edged in gold.
    let label_em = 0.62 * em;
    let label = if plate.triggered {
        "TRIGGERED ABILITY"
    } else {
        "ACTIVATED ABILITY"
    };
    let label = text.set(
        label,
        TextStyle {
            font: Font::CrimsonPro,
            weight: 650.0,
            size: label_em,
            line_height: 1.1,
            color: [0xf2, 0xea, 0xd8],
        },
        None,
        false,
    );
    let tab = Box2 {
        x: area.x + (area.width - label.width) / 2.0 - 0.6 * label_em,
        y: area.y + 0.45 * label_em,
        width: label.width + 1.2 * label_em,
        height: label.height + 0.2 * label_em,
    };
    canvas.fill_box(
        tab,
        tab.height / 2.0,
        Shader::SolidColor(color([0xc9, 0xa3, 0x5c], 1.0)),
    );
    let inner = tab.inset(
        0.08 * label_em,
        0.08 * label_em,
        0.08 * label_em,
        0.08 * label_em,
    );
    let dark = vertical(
        inner,
        stops(&[
            (0.0, [0x3a, 0x36, 0x31], 1.0),
            (1.0, [0x1c, 0x1a, 0x17], 1.0),
        ]),
    );
    canvas.fill_box(inner, inner.height / 2.0, dark);
    canvas.text(&label, (tab.x + 0.6 * label_em, tab.y + 0.1 * label_em));
    // The words: `.ability-plate-text`, 0.74em, padded 0.3em by 0.55em in that em.
    let size = 0.74 * em;
    let room = Box2 {
        x: area.x,
        y: tab.bottom(),
        width: area.width,
        height: area.bottom() - tab.bottom(),
    }
    .inset(0.3 * size, 0.55 * size, 0.3 * size, 0.55 * size);
    let style = TextStyle {
        font: Font::CrimsonPro,
        weight: 560.0,
        size,
        line_height: 1.22,
        color: INK,
    };
    let set = text.set_block_to_fit(&plate.rules, style, (room.width, room.height));
    let top = room.y + ((room.height - set.height) / 2.0).max(0.0);
    canvas.text(&set, (room.x, top));
}
