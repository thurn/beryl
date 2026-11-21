//! The shapes of light: the atlas's page 0, which glows, streaks, marks and effects draw from
//! (docs/beryl-3d.md, "Effects", "The shapes of light"). It is drawn here in code, with no
//! image file and no randomness, so every machine draws the same page. Each shape fills one
//! cell of `kit_frame::frame::sprites`, white with its shape in the opacity, like a glyph
//! page's coverage, and leaves an empty border of `BORDER` texels around itself so that a
//! sampler filtering at the cell's edge never reads its neighbour.
//!
//! The glyph worker's pages start at page 1 (`glyphs`), and a host uploads this page before
//! its first frame.

use kit_base::atlas::AssetReady;
use kit_base::data::light::{Glow, Light, Own};
use kit_frame::frame::{Sprite, sprites};

use crate::packer::PAGE_SIZE;
use crate::{Prepared, Upload};

/// The empty texels around each shape in its cell.
pub const BORDER: u16 = 2;

/// Half the side of a cell's drawn area, in texels: the area is `CELL - 2 * BORDER` square.
const HALF: f32 = (sprites::CELL - 2 * BORDER) as f32 / 2.0;
/// One texel, in the coordinates a shape is drawn in, which run from -1 to 1 across the
/// drawn area.
const TEXEL: f32 = 1.0 / HALF;

/// The atlas's page 0, `PAGE_SIZE` rows of as many 8-bit opacity texels, top row first.
#[must_use]
pub fn light_page(light: &Light, own: &Own) -> Vec<u8> {
    let side = usize::from(PAGE_SIZE);
    let mut pixels = vec![0; side * side];
    let tuning = Tuning {
        glow: &light.glow,
        chevron: own.wedge.chevron,
    };
    for (sprite, shape) in SHAPES {
        draw(&mut pixels, sprite, (&tuning, shape));
    }
    pixels
}

/// The page as a host uploads it, through `Renderer::upload_page` as it does a glyph page.
/// Its `ready` lists no glyphs, and the client needs nothing from it: the cells of `sprites`
/// are constants.
#[must_use]
pub fn prepared(light: &Light, own: &Own) -> Prepared {
    Prepared {
        upload: Upload::AtlasPage {
            page: sprites::PAGE,
            pixels: light_page(light, own),
        },
        ready: AssetReady::Glyphs {
            page: sprites::PAGE,
            glyphs: Vec::new(),
        },
    }
}

/// The tuning the shapes of light are drawn from: the glow of the ribbons, and how far the
/// attacker's chevron reaches past its card, in card widths. The scene reads the same values
/// when it sizes and lights the records that sample these shapes.
#[derive(Clone, Copy)]
struct Tuning<'a> {
    glow: &'a Glow,
    chevron: f32,
}

/// A shape: given the tuning and a point of its cell, x right and y up, each from -1 to 1
/// across the drawn area, its opacity there from 0 to 1.
type Shape = fn(&Tuning<'_>, f32, f32) -> f32;

/// Each shape with its cell.
const SHAPES: [(Sprite, Shape); 16] = [
    (sprites::DISC, |_, x, y| disc(x, y)),
    (sprites::BURST, |_, x, y| burst(x, y)),
    (sprites::RING, |_, x, y| ring(x, y)),
    (sprites::CONE, |_, x, y| cone(x, y)),
    (sprites::COMET, |_, x, y| comet(x, y)),
    (sprites::RIBBON_CELL, ribbons),
    (sprites::OUTLINE, |_, x, y| outline(x, y)),
    (sprites::HEAD, |_, x, y| head(x, y)),
    (sprites::WEDGE, wedge),
    (sprites::CRESCENT, |_, x, y| crescent(x, y)),
    (sprites::TAP, |_, x, y| tap(x, y)),
    (sprites::MOON, |_, x, y| moon(x, y)),
    (sprites::PLAQUE, |_, x, y| plaque(x, y)),
    (sprites::COIN, |_, x, y| coin(x, y)),
    (sprites::HALO, |_, x, y| halo(x, y)),
    (sprites::WEDGE_HOT, wedge_hot),
];

/// Fills `sprite`'s cell of `pixels` with `shape`, leaving its border empty.
fn draw(pixels: &mut [u8], sprite: Sprite, (tuning, shape): (&Tuning<'_>, Shape)) {
    let [left, top, width, height] = sprite.rect;
    let side = usize::from(PAGE_SIZE);
    for row in BORDER..height - BORDER {
        for column in BORDER..width - BORDER {
            // The texel's centre, from -1 to 1 across the drawn area, y up.
            let x = (f32::from(column - BORDER) + 0.5) / HALF - 1.0;
            let y = 1.0 - (f32::from(row - BORDER) + 0.5) / HALF;
            let opacity = shape(tuning, x, y).clamp(0.0, 1.0);
            let index = usize::from(top + row) * side + usize::from(left + column);
            pixels[index] = (opacity * 255.0).round() as u8;
        }
    }
}

/// 1 inside a shape whose signed distance is `distance` (negative inside), 0 outside, with
/// its edge antialiased over one texel.
fn fill(distance: f32) -> f32 {
    (0.5 - distance / TEXEL).clamp(0.0, 1.0)
}

/// 1 at 0, falling smoothly to 0 at 1 and beyond.
fn falloff(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// Signed distance to a rounded rectangle centred on the origin, `half` its half size.
fn rounded_box(x: f32, y: f32, half: (f32, f32), radius: f32) -> f32 {
    let qx = x.abs() - half.0 + radius;
    let qy = y.abs() - half.1 + radius;
    length(qx.max(0.0), qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// Signed distance to the triangle `a`, `b`, `c`, whose points run counterclockwise.
fn triangle(p: (f32, f32), [a, b, c]: [(f32, f32); 3]) -> f32 {
    // The largest distance outside any of the three edges, each edge's outward side being on
    // its right.
    [(a, b), (b, c), (c, a)]
        .iter()
        .map(|&(from, to)| {
            let (ex, ey) = (to.0 - from.0, to.1 - from.1);
            let along = length(ex, ey);
            ((p.0 - from.0) * ey - (p.1 - from.1) * ex) / along
        })
        .fold(f32::MIN, f32::max)
}

/// A soft disc: full at the centre, its opacity falling smoothly to nothing at the edge and
/// never rising on the way out.
fn disc(x: f32, y: f32) -> f32 {
    let soft = falloff(length(x, y));
    soft * soft
}

/// A burst, as the web stage's light painter draws one (`u_kind == 4` in
/// `ui/src/stage/glow.ts`): a white-hot core, a wide soft bloom and rays, all fading out before
/// the edge of the cell. The web adds them up at a gain of about 2 for a blow, a spell's flare
/// and a permanent landing, and soft-clips the sum (`lit`, `1 - exp(-1.5 x)`), so that the
/// middle burns to full and the bloom stays bright well out from it. The sprite holds that
/// soft-clipped light at a gain of 2; a record's tint and progress fade it from there. The web
/// picks its rays from noise; these are 12 thin rays and 5 broad ones, fixed, so that every
/// machine draws the same page.
fn burst(x: f32, y: f32) -> f32 {
    /// The gain the web draws a burst at as it is brightest.
    const GAIN: f32 = 2.0;
    let r = length(x, y);
    let angle = y.atan2(x);
    let fine = (12.0 * angle).cos().max(0.0).powi(16);
    let broad = (0.5 + 0.5 * (5.0 * angle + 1.0).cos()).powi(3);
    let spikes = fine * 1.2 + broad * 0.4;
    let edge = 1.0 - smooth(0.6, 1.0, r);
    let core = (-r * r * 13.0).exp();
    let bloom = (-r * r * 4.5).exp() * 0.55;
    let rays = spikes * (-r * 3.4).exp() * smooth(0.04, 0.2, r);
    // The web's hot color and its tone, both near white for these, summed as one light.
    let light = (core * 1.5 + bloom * 1.6 + rays * 1.35) * edge;
    1.0 - (-1.5 * GAIN * light).exp()
}

/// 0 at `from` and below, 1 at `to` and above, rising smoothly between, as GLSL's `smoothstep`.
fn smooth(from: f32, to: f32, t: f32) -> f32 {
    let t = ((t - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A ring three quarters of the way out, soft on both sides, empty at the centre.
fn ring(x: f32, y: f32) -> f32 {
    falloff((length(x, y) - 0.75).abs() / 0.18)
}

/// A cone of light from a point at the bottom, widening upward and fading toward its far end
/// and its sides.
fn cone(x: f32, y: f32) -> f32 {
    let up = y + 1.0;
    if up <= 0.0 {
        return 0.0;
    }
    // The half angle is about 27 degrees: at the top the cone is as wide as the cell.
    let across = x.abs() / (up * 0.5);
    falloff(across) * falloff(up / 2.0).sqrt()
}

/// A comet flying right: a bright head with a tail behind it that thins and fades.
fn comet(x: f32, y: f32) -> f32 {
    const HEAD_X: f32 = 0.65;
    const HEAD_RADIUS: f32 = 0.3;
    let head = falloff(length(x - HEAD_X, y) / HEAD_RADIUS);
    let behind = (HEAD_X - x).max(0.0) / (HEAD_X + 1.0);
    let tail = if x < HEAD_X {
        let width = HEAD_RADIUS * (1.0 - behind);
        if width > 0.0 {
            falloff(y.abs() / width) * (1.0 - behind)
        } else {
            0.0
        }
    } else {
        0.0
    };
    head.max(tail)
}

/// The ribbons' cell (`sprites::RIBBON_CELL`): side by side, `sprites::RIBBON`, a soft band,
/// `sprites::RIBBON_CORE`, an arrow's hot core, and the columns of `sprites::ribbon_lit`, each
/// a ribbon's body and halo. Each is the same all along, so a record draws its profile across
/// whatever stretch of the ribbon its quad covers.
fn ribbons(tuning: &Tuning<'_>, x: f32, y: f32) -> f32 {
    // The texel column `x` is the middle of, counted from the cell's left edge.
    let column = (x + 1.0) * HALF + f32::from(BORDER) - 0.5;
    let first = f32::from(sprites::RIBBON_LIT_FIRST);
    if column >= first {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a column of the cell, from 0 to 255"
        )]
        let lit = (column - first).round() as u16;
        if lit >= sprites::RIBBON_LIT_COLUMNS {
            return 0.0;
        }
        return ribbon_lit(tuning.glow, sprites::ribbon_lit_share(lit), y.abs());
    }
    let core = f32::from(sprites::RIBBON_CORE.rect[0] - sprites::RIBBON_CELL.rect[0]);
    if column >= core - 8.0 {
        return ribbon_core(tuning.glow, y.abs());
    }
    falloff(y.abs())
}

/// Soft-clipped light, as the web stage's `lit` takes a sum of light (`ui/src/stage/glow.ts`):
/// `1 - exp(-1.5 x)`, which burns what is past full toward white as a bloom does. A shape of
/// light holds its light soft-clipped at the gain `light.glow.gain`, and a record's tint
/// fades it from there.
fn soft_clipped(glow: &Glow, light: f32) -> f32 {
    1.0 - (-1.5 * glow.gain * light).exp()
}

/// A ribbon's body and halo `across` from its middle, in shares of its quad's half height, where
/// its body is `share` of that height, as the web's ribbon shader lights them in its tone's
/// color: a band with soft edges out to a little past the body's width (`body`), and a halo
/// that falls off from the middle as `exp(-d / (0.9 b + 3.25))` (`glow`), with `b` the body's
/// half-width and `d` the distance from the middle, in web stage pixels. The quad is
/// `2.6 b + 16` pixels high on each side, as the web's strip is, so the halo's scale is a share
/// of that height that depends on `share` alone. The web's noise along the ribbon averages
/// about 1, and its edge here is faded over the last tenth of the quad.
fn ribbon_lit(glow: &Glow, share: f32, across: f32) -> f32 {
    // From `share = b / h` and `h = reach_share b + reach_pixels`: the pixel, in shares of h.
    let pixel = ((1.0 - glow.reach_share * share) / glow.reach_pixels).max(0.0);
    let halo_scale = glow.halo_share * share + glow.halo_pixels * pixel;
    let body = if share > 0.0 {
        1.0 - smooth(0.4, 1.08, across / share)
    } else {
        0.0
    };
    let halo = (-across / halo_scale).exp();
    let edge = 1.0 - smooth(0.9, 1.0, across);
    soft_clipped(glow, glow.body * body + glow.halo * halo) * edge
}

/// An arrow's hot core `across` from its middle, in shares of its quad's half height, which is
/// `light.glow.core_reach` times the body's half-width: the web's `exp(-(2.5 x)^2)`, with `x`
/// the distance in body half-widths, at the strength of its noise's average.
fn ribbon_core(glow: &Glow, across: f32) -> f32 {
    let x = across * glow.core_reach;
    soft_clipped(glow, glow.core * (-(2.5 * x).powi(2)).exp()) * (1.0 - smooth(0.9, 1.0, across))
}

/// A card-shaped outline: the ring of a rounded rectangle of a card's proportions, solid on
/// the line and soft at its sides, empty inside.
fn outline(x: f32, y: f32) -> f32 {
    // `sprites::OUTLINE_LINE` is in units of the whole cell, -0.5 to 0.5 across, and a shape is
    // drawn in units of the drawn area, -1 to 1 across.
    let [width, height] = sprites::OUTLINE_LINE.map(|half| half * f32::from(sprites::CELL) / HALF);
    let half = (width, height);
    // A card's 3 millimetre corner, at the card's 88 millimetre height.
    let radius = height * 2.0 * 3.0 / 88.0;
    let distance = rounded_box(x, y, half, radius).abs();
    falloff((distance - 0.02).max(0.0) / 0.04)
}

/// The halo of the `glow` effect round a card, as the two box shadows of `.stage-effect-glow`
/// in `ui/src/stage/effects.css` light it, with distances in card widths (its `--frame`): a rim
/// of light spread 0.03 out from the card's edge and blurred by 0.03, over a warm halo spread
/// 0.1 out and blurred by 0.22 at 75% opacity. A CSS blur is a Gaussian whose standard
/// deviation is half its radius. Empty inside the card, as a shadow is under its box, and
/// faded to nothing before the edge of its cell.
fn halo(x: f32, y: f32) -> f32 {
    let [width, height] = sprites::HALO_CARD.map(|half| half * f32::from(sprites::CELL) / HALF);
    // A card's 3 millimetre corner, at the card's 88 millimetre height.
    let radius = height * 2.0 * 3.0 / 88.0;
    let outside = rounded_box(x, y, (width, height), radius);
    if outside <= 0.0 {
        return 0.0;
    }
    let card_widths = outside / (2.0 * width);
    let shadow = |spread: f32, blur: f32| {
        let sigma = blur * 0.5;
        let past = (card_widths - spread).max(0.0) / sigma;
        (-0.5 * past * past).exp()
    };
    let rim = shadow(0.03, 0.03);
    let warm = 0.75 * shadow(0.1, 0.22);
    let edge = falloff(((length(x, y).max(x.abs().max(y.abs())) - 0.85) / 0.15).max(0.0));
    (1.0 - (1.0 - rim) * (1.0 - warm)) * edge
}

/// An arrowhead pointing up.
fn head(x: f32, y: f32) -> f32 {
    fill(triangle((x, y), [(0.0, 0.9), (-0.8, -0.7), (0.8, -0.7)])).min(fill(-triangle(
        (x, y),
        [(0.0, -0.3), (-0.81, -0.71), (0.81, -0.71)],
    )))
}

/// The chevron an attacker wears past its card's edge, as the web stage's light painter draws
/// it (`u_kind == 1` in `ui/src/stage/glow.ts`): a triangle with its base along the card's edge
/// and its point `chevron` card widths past it, with a rim of light along its two outer edges,
/// a fill that is brightest toward the point and fades toward the card, and a halo round it,
/// all dimmer toward its two ends. The quad it fills is `sprites::wedge_size` large, so the
/// triangle is drawn in card widths inside room of `sprites::WEDGE_ROOM` on every side, at the
/// desktop table's scale of about 95 points to a card's width. This record holds the light
/// the web gives the tone's glow color, soft-clipped as its `lit` clips it; `wedge_hot` holds
/// the light it gives the core color.
fn wedge(tuning: &Tuning<'_>, x: f32, y: f32) -> f32 {
    let parts = Chevron::at(tuning.chevron, x, y);
    let light = (parts.fill * 1.7 + parts.glow + parts.line) * parts.arm;
    1.0 - (-1.5 * light).exp()
}

/// The white-hot part of the chevron that `wedge` draws: its rim, hottest at the point, and
/// the fill near the point, in the web's tone's core color.
fn wedge_hot(tuning: &Tuning<'_>, x: f32, y: f32) -> f32 {
    let parts = Chevron::at(tuning.chevron, x, y);
    let light =
        (parts.line * (0.65 + 0.9 * parts.apex) + parts.fill * parts.apex * 0.55) * parts.arm;
    1.0 - (-1.5 * light).exp()
}

/// The parts of the web's chevron at one point of the cell, each from 0 to about 1.
struct Chevron {
    fill: f32,
    glow: f32,
    line: f32,
    apex: f32,
    arm: f32,
}

impl Chevron {
    /// The parts at `(x, y)`, each from -1 to 1 across the cell's drawn area with y toward the
    /// point.
    fn at(chevron: f32, x: f32, y: f32) -> Self {
        /// One point of the web's desktop table, in card widths.
        const POINT: f32 = 1.0 / 95.0;
        let room = sprites::WEDGE_ROOM;
        // In card widths, from the middle of the base: the base is a card wide, and the point
        // is `chevron` past it.
        let px = x * 0.5 * (1.0 + 2.0 * room);
        let py = f32::midpoint(y, 1.0) * (chevron + 2.0 * room) - room;
        let (half, high) = (0.5, chevron);
        let edge_y = high * (1.0 - px.abs() / half);
        let distance = segment((px, py), (-half, 0.0), (0.0, high)).min(segment(
            (px, py),
            (half, 0.0),
            (0.0, high),
        ));
        let inside = py >= 0.0 && py <= edge_y && px.abs() <= half;
        let fill = if inside {
            (py / edge_y.max(POINT)).clamp(0.0, 1.0).powf(1.25) * 0.95
        } else {
            0.0
        };
        // The web's rim is `max(2.2, 0.03 * half + 0.8 * pixel)` points wide, its halo fades
        // over `0.09 * half + 4` points, with `half` the half width in points.
        let half_points = half / POINT;
        let width = 2.2f32.max(0.03 * half_points + 0.8) * POINT;
        let line = (-distance * distance / (2.0 * width * width)).exp();
        let glow = (-distance / ((0.09 * half_points + 4.0) * POINT)).exp() * 0.55;
        let apex = (-length(px, py - high) / (high * 0.85)).exp();
        let arm = 0.6 + 0.4 * (1.0 - smooth(half * 0.25, half, px.abs()));
        // Fade out before the edge of the cell, so the halo never ends in a hard line.
        let edge = falloff(((x.abs().max(y.abs()) - 0.85) / 0.15).max(0.0));
        Self {
            fill: fill * edge,
            glow: glow * edge,
            line: line * edge,
            apex,
            arm,
        }
    }
}

/// The distance from `p` to the segment from `a` to `b`.
fn segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (pa, ba) = ((p.0 - a.0, p.1 - a.1), (b.0 - a.0, b.1 - a.1));
    let t = ((pa.0 * ba.0 + pa.1 * ba.1) / (ba.0 * ba.0 + ba.1 * ba.1)).clamp(0.0, 1.0);
    length(pa.0 - ba.0 * t, pa.1 - ba.1 * t)
}

/// A thin crescent: a disc with an offset disc cut out of it, opening right.
fn crescent(x: f32, y: f32) -> f32 {
    let outer = length(x, y) - 0.9;
    let inner = length(x - 0.35, y) - 0.78;
    fill(outer.max(-inner))
}

/// A tap mark: an arc from the left over the top to the right, ending in an arrowhead that
/// points down, the way a card turns when it is tapped.
fn tap(x: f32, y: f32) -> f32 {
    const RADIUS: f32 = 0.55;
    const HALF_WIDTH: f32 = 0.09;
    // The arc covers the upper half, from the left end over the top to the right end. Below
    // the middle only its flat ends' antialiased edges remain.
    let ring = (length(x, y) - RADIUS).abs() - HALF_WIDTH;
    let band = if y >= 0.0 { ring } else { ring.max(-y) };
    let arrow = triangle(
        (x, y),
        [
            (RADIUS, -0.45),
            (RADIUS + 0.25, 0.02),
            (RADIUS - 0.25, 0.02),
        ],
    );
    fill(band.min(arrow))
}

/// A moon: a wide crescent, mostly disc, opening right.
fn moon(x: f32, y: f32) -> f32 {
    let outer = length(x, y) - 0.9;
    let inner = length(x - 0.6, y + 0.15) - 0.6;
    fill(outer.max(-inner))
}

/// A plaque: a filled rounded rectangle, wider than it is tall.
fn plaque(x: f32, y: f32) -> f32 {
    fill(rounded_box(x, y, (0.95, 0.5), 0.2))
}

/// A coin: a filled disc with a fainter groove inside its rim.
fn coin(x: f32, y: f32) -> f32 {
    let r = length(x, y);
    let groove = fill(((r - 0.74).abs()) - 0.04);
    fill(r - 0.9) * (1.0 - 0.4 * groove)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cell_of_sprites_has_a_shape() {
        let mut cells: Vec<_> = SHAPES.iter().map(|(sprite, _)| sprite.rect).collect();
        cells.sort_unstable();
        cells.dedup();
        assert_eq!(cells.len(), SHAPES.len());
        assert!(
            SHAPES
                .iter()
                .all(|(sprite, _)| sprite.page == sprites::PAGE)
        );
    }
}
