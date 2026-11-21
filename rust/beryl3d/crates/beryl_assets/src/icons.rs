//! Rasterizes SVG icons and their drop-shadow chains into atlas coverage.

use std::cell::Cell;

use kit_base::atlas::artwork::Artwork;
use kit_base::atlas::shadows;
use kit_base::paint::Shadow;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Pixmap, Stroke, Transform};

use crate::blur;
use crate::glyphs::Bitmap;
use crate::svg;

thread_local! {
    static ICONS_RASTERIZED: Cell<u64> = const { Cell::new(0) };
}

/// How many icons this thread has rasterized. Only the glyph worker rasterizes them, so on any
/// other thread, such as the one that draws frames, this stays zero.
#[must_use]
pub fn icons_rasterized_on_this_thread() -> u64 {
    ICONS_RASTERIZED.with(Cell::get)
}

/// Draws `artwork` at `scale` physical pixels per point into a square of coverage. A shape
/// whose path data does not parse draws nothing.
#[must_use]
pub(crate) fn rasterize(artwork: &Artwork, scale: f32) -> Bitmap {
    draw(artwork, scale, false)
}

/// The cells of the drop shadows `shadows` of `artwork` at
/// `scale`, one for each shadow, as CSS applies a chain: shadow `k`'s cell is the coverage of
/// the icon and every shadow before it, composited, blurred by shadow `k`'s deviation
/// (`Shadow::drop_sigma`) and moved by its offset. Each cell is the icon's square grown by
/// `shadows::stage_pad` pixels on every side, in texels of `stage_step` pixels, each the
/// mean of the pixels it covers. A drop shadow is cast by the icon's alpha, not its colors, so
/// every fill is drawn at full coverage first, whatever its `ink`.
#[must_use]
pub(crate) fn rasterize_shadows(artwork: &Artwork, scale: f32, shadows: &[Shadow]) -> Vec<Bitmap> {
    let Some(last) = shadows.len().checked_sub(1) else {
        return Vec::new();
    };
    let icon = draw(artwork, scale, true);
    let side = usize::from(icon.width);
    let pad = usize::from(shadows::stage_pad(shadows, last, scale));
    let wide = side + 2 * pad;
    // The alpha of the icon and the shadows composited so far, on a canvas grown by the last
    // shadow's pad.
    let mut alpha = vec![0.0_f32; wide * wide];
    for y in 0..side {
        for x in 0..side {
            alpha[(y + pad) * wide + x + pad] = f32::from(icon.coverage[y * side + x]) / 255.0;
        }
    }
    let mut cells = Vec::with_capacity(shadows.len());
    for (stage, shadow) in shadows.iter().enumerate() {
        let blurred = blur::blur_plane(&alpha, wide, shadow.drop_sigma() * scale);
        let moved = moved_by(&blurred, wide, [shadow.x * scale, shadow.y * scale]);
        let opacity = f32::from(shadow.color[3]) / 255.0;
        for (under, &over) in alpha.iter_mut().zip(&moved) {
            *under += opacity * over * (1.0 - *under);
        }
        let reach = usize::from(shadows::stage_pad(shadows, stage, scale));
        let step = usize::from(shadows::stage_step(shadows, stage, scale));
        cells.push(cropped(&moved, wide, pad - reach, side + 2 * reach, step));
    }
    cells
}

/// `plane`, rows of `wide` values, moved by `by` pixels rounded to whole ones, with nothing
/// coming in past its edges.
fn moved_by(plane: &[f32], wide: usize, by: [f32; 2]) -> Vec<f32> {
    #[expect(clippy::cast_possible_truncation, reason = "an offset of a few pixels")]
    let [dx, dy] = by.map(|length| length.round() as isize);
    let tall = plane.len() / wide.max(1);
    let mut out = vec![0.0_f32; plane.len()];
    for y in 0..tall {
        for x in 0..wide {
            let from = (y.checked_add_signed(-dy), x.checked_add_signed(-dx));
            if let (Some(from_y), Some(from_x)) = from
                && from_y < tall
                && from_x < wide
            {
                out[y * wide + x] = plane[from_y * wide + from_x];
            }
        }
    }
    out
}

/// The square of `plane` (rows of `wide` values) `length` pixels across with its top left
/// corner at `start` on both axes, as a cell of coverage in texels of `step` by `step` pixels,
/// each the mean of the pixels it covers, a texel past the square's edge counting the pixels
/// beyond as empty.
fn cropped(plane: &[f32], wide: usize, start: usize, length: usize, step: usize) -> Bitmap {
    let step = step.max(1);
    let texels = length.div_ceil(step);
    let mut coverage = vec![0_u8; texels * texels];
    #[expect(clippy::cast_precision_loss, reason = "a small count")]
    let block = (step * step) as f32;
    for row in 0..texels {
        for column in 0..texels {
            let mut sum = 0.0;
            for y in row * step..((row + 1) * step).min(length) {
                for x in column * step..((column + 1) * step).min(length) {
                    sum += plane[(start + y) * wide + start + x];
                }
            }
            coverage[row * texels + column] = blur::level(sum / block);
        }
    }
    let side = u16::try_from(texels).unwrap_or(u16::MAX);
    Bitmap {
        width: side,
        height: side,
        bearing: [0, 0],
        coverage,
    }
}

/// Draws `artwork` as `rasterize` does, or with every fill at full coverage if `solid`.
fn draw(artwork: &Artwork, scale: f32, solid: bool) -> Bitmap {
    ICONS_RASTERIZED.with(|count| count.set(count.get() + 1));
    let side = artwork.side(scale);
    let mut pixmap = Pixmap::new(u32::from(side), u32::from(side)).expect("a cell has an area");
    let [left, top, width, height] = artwork.view;
    let fit = f32::from(side) / width.max(height);
    let base = Transform::from_row(
        fit,
        0.0,
        0.0,
        fit,
        (f32::from(side) - width * fit) / 2.0 - left * fit,
        (f32::from(side) - height * fit) / 2.0 - top * fit,
    );
    for shape in artwork.shapes {
        let Some(path) = svg::parse(shape.data) else {
            continue;
        };
        let transform = shape.rotate.map_or(base, |[degrees, x, y]| {
            base.pre_concat(Transform::from_rotate_at(degrees, x, y))
        });
        if let Some(fill) = shape.fill {
            let rule = if fill.even_odd {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
            let ink = if solid { 1.0 } else { fill.ink };
            pixmap.fill_path(&path, &paint(ink), rule, transform, None);
        }
        if let Some(stroke) = shape.stroke {
            let stroke = Stroke {
                width: stroke.width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &paint(1.0), &stroke, transform, None);
        }
    }
    // The pixels are premultiplied, so a gray of `ink` painted opaque leaves its red channel
    // at `ink` times the coverage.
    let coverage = pixmap.pixels().iter().map(|pixel| pixel.red()).collect();
    Bitmap {
        width: side,
        height: side,
        bearing: [0, 0],
        coverage,
    }
}

/// An opaque gray of brightness `ink`, antialiased.
fn paint(ink: f32) -> Paint<'static> {
    let gray = (ink.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut paint = Paint::default();
    paint.set_color_rgba8(gray, gray, gray, 255);
    paint.anti_alias = true;
    paint
}
