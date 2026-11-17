//! The shapes of light: the atlas's page 0, which glows, streaks, marks and effects draw from
//! (docs/beryl-3d.md, "Effects", "The shapes of light"). It is drawn here in code, with no
//! image file and no randomness, so every machine draws the same page. Each shape fills one
//! cell of `beryl_scene::frame::sprites`, white with its shape in the opacity, like a glyph
//! page's coverage, and leaves an empty border of `BORDER` texels around itself so that a
//! sampler filtering at the cell's edge never reads its neighbour.
//!
//! The glyph worker's pages start at page 1 (`glyphs`), and a host uploads this page before
//! its first frame.

use beryl_scene::frame::{Sprite, sprites};
use beryl_scene::input::AssetReady;

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
pub fn light_page() -> Vec<u8> {
    let side = usize::from(PAGE_SIZE);
    let mut pixels = vec![0; side * side];
    for (sprite, shape) in SHAPES {
        draw(&mut pixels, sprite, shape);
    }
    pixels
}

/// The page as a host uploads it, through `Renderer::upload_page` as it does a glyph page.
/// Its `ready` lists no glyphs, and the client needs nothing from it: the cells of `sprites`
/// are constants.
#[must_use]
pub fn prepared() -> Prepared {
    Prepared {
        upload: Upload::AtlasPage {
            page: sprites::PAGE,
            pixels: light_page(),
        },
        ready: AssetReady::Glyphs {
            page: sprites::PAGE,
            glyphs: Vec::new(),
        },
    }
}

/// A shape: given a point of its cell, x right and y up, each from -1 to 1 across the drawn
/// area, its opacity there from 0 to 1.
type Shape = fn(f32, f32) -> f32;

/// Each shape with its cell.
const SHAPES: [(Sprite, Shape); 14] = [
    (sprites::DISC, disc),
    (sprites::BURST, burst),
    (sprites::RING, ring),
    (sprites::CONE, cone),
    (sprites::COMET, comet),
    (sprites::RIBBON, ribbon),
    (sprites::OUTLINE, outline),
    (sprites::HEAD, head),
    (sprites::WEDGE, wedge),
    (sprites::CRESCENT, crescent),
    (sprites::TAP, tap),
    (sprites::MOON, moon),
    (sprites::PLAQUE, plaque),
    (sprites::COIN, coin),
];

/// Fills `sprite`'s cell of `pixels` with `shape`, leaving its border empty.
fn draw(pixels: &mut [u8], sprite: Sprite, shape: Shape) {
    let [left, top, width, height] = sprite.rect;
    let side = usize::from(PAGE_SIZE);
    for row in BORDER..height - BORDER {
        for column in BORDER..width - BORDER {
            // The texel's centre, from -1 to 1 across the drawn area, y up.
            let x = (f32::from(column - BORDER) + 0.5) / HALF - 1.0;
            let y = 1.0 - (f32::from(row - BORDER) + 0.5) / HALF;
            let opacity = shape(x, y).clamp(0.0, 1.0);
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

/// A burst: a small bright core with 12 thin rays that fade toward their ends.
fn burst(x: f32, y: f32) -> f32 {
    let r = length(x, y);
    let angle = y.atan2(x);
    let ray = (12.0 * angle).cos().max(0.0).powi(24) * falloff(r);
    let core = falloff(r / 0.3);
    ray.max(core)
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

/// A ribbon running left to right: soft across, the same all along.
fn ribbon(_x: f32, y: f32) -> f32 {
    falloff(y.abs())
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

/// An arrowhead pointing up.
fn head(x: f32, y: f32) -> f32 {
    fill(triangle((x, y), [(0.0, 0.9), (-0.8, -0.7), (0.8, -0.7)])).min(fill(-triangle(
        (x, y),
        [(0.0, -0.3), (-0.81, -0.71), (0.81, -0.71)],
    )))
}

/// A wedge: a slice of a disc, 60 degrees wide, opening upward from its point near the bottom
/// of the cell.
fn wedge(x: f32, y: f32) -> f32 {
    let y = y + 0.9;
    let (sin, cos) = std::f32::consts::FRAC_PI_6.sin_cos();
    // Distance outside each straight side, and outside the arc.
    let sides = (-x * cos - y * sin).max(x * cos - y * sin);
    fill(sides.max(length(x, y) - 1.8))
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
