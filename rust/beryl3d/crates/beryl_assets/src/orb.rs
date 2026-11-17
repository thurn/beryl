//! The layers of the life orbs' art (`beryl_scene::art::orb`) drawn into coverage, one atlas
//! cell for each layer of each seat's orb at each scale the orb is drawn at. The glyph worker
//! draws them with the icons (`GlyphWorker::icons`), and the flat layer tints each cell with its
//! layer's color.
//!
//! A cell covers its layer's box (`Layer::views`) and is a whole number of points wide
//! (`orb::cell_width`), so the flat layer can key it by its icon and width as it keys an icon.
//! Into it go the layer's outlines, filled and stroked with `tiny_skia`, then its fade
//! (`Ramp`), multiplied into the coverage, then its blur.

use beryl_scene::art::orb::{self, Layer, Ramp};
use beryl_scene::input::IconCell;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::glyphs::Bitmap;

/// One cell of the art: a layer of one seat's orb at one scale.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// Whose orb, by index in `orb::SIDES`: the viewer's first.
    pub seat: usize,
    /// The layer's index in `orb::LAYERS`.
    pub at: usize,
    /// Logical points to a unit of the art.
    pub units: f32,
}

impl Cell {
    fn layer(self) -> &'static Layer {
        &orb::LAYERS[self.at]
    }

    /// The layer's box on the screen, in units.
    fn view(self) -> [f32; 4] {
        self.layer().views[self.seat]
    }

    /// The cell's width in logical points.
    fn width(self) -> u16 {
        orb::cell_width(self.view()[2], self.units)
    }

    /// The cell's size in texels at `scale` physical pixels per point: its width, and its
    /// height in proportion to its box.
    fn texels(self, scale: f32) -> (u16, u16) {
        let [_, _, w, h] = self.view();
        let width = f32::from(self.width()) * scale;
        let height = width * h / w;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to a page"
        )]
        let size = |texels: f32| texels.round().clamp(1.0, 1024.0) as u16;
        (size(width), size(height))
    }
}

/// Every cell of the art: each layer of each seat's orb at each scale.
pub fn cells() -> impl Iterator<Item = Cell> {
    orb::SCALES.into_iter().flat_map(|units| {
        (0..orb::SIDES.len())
            .flat_map(move |seat| (0..orb::LAYERS.len()).map(move |at| Cell { seat, at, units }))
    })
}

/// Draws `cell` at `scale` physical pixels per point into coverage.
#[must_use]
pub(crate) fn rasterize(cell: Cell, scale: f32) -> Bitmap {
    let layer = cell.layer();
    let (width, height) = cell.texels(scale);
    let mut pixmap = Pixmap::new(u32::from(width), u32::from(height)).expect("a cell has an area");
    let [left, top, w, h] = cell.view();
    let (across, down) = (f32::from(width) / w, f32::from(height) / h);
    let transform = Transform::from_row(
        across,
        0.0,
        0.0,
        down,
        -left * across,
        (layer.drop - top) * down,
    );
    let mut builder = PathBuilder::new();
    for (points, closed) in orb::outlines(layer.shape, orb::SIDES[cell.seat]) {
        let mut points = points.into_iter();
        let Some(first) = points.next() else {
            continue;
        };
        builder.move_to(first.x, first.y);
        for point in points {
            builder.line_to(point.x, point.y);
        }
        if closed {
            builder.close();
        }
    }
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    if let Some(path) = builder.finish() {
        if layer.paint.fill {
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
        if let Some((stroke_width, round)) = layer.paint.stroke {
            let stroke = Stroke {
                width: stroke_width,
                line_cap: if round { LineCap::Round } else { LineCap::Butt },
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
        }
    }
    let mut coverage: Vec<u8> = pixmap.pixels().iter().map(|pixel| pixel.alpha()).collect();
    fade(&mut coverage, cell, (width, height));
    // The blur's standard deviation in texels.
    let sigma = layer.blur * across;
    if sigma > 0.0 {
        blur(&mut coverage, (width, height), sigma);
    }
    Bitmap {
        width,
        height,
        bearing: [0, 0],
        coverage,
    }
}

/// Multiplies `coverage`, a cell of `size` texels, by `cell`'s layer's ramp at each texel's
/// centre.
fn fade(coverage: &mut [u8], cell: Cell, (width, height): (u16, u16)) {
    let layer = cell.layer();
    let [left, top, w, h] = cell.view();
    let alpha: Box<dyn Fn(f32, f32) -> f32> = match layer.ramp {
        Ramp::None => return,
        Ramp::Down([rise, whole, keep, gone]) => {
            let core_top = orb::core_top(orb::SIDES[cell.seat]);
            let height = 2.0 * orb::CORE_RY;
            Box::new(move |_, y| {
                let at = (y - core_top) / height;
                if at < rise || at > gone {
                    0.0
                } else if at < whole {
                    (at - rise) / (whole - rise)
                } else if at > keep {
                    (gone - at) / (gone - keep)
                } else {
                    1.0
                }
            })
        }
        Ramp::Out { whole, gone } => Box::new(move |x, y| {
            let r = x.hypot(y);
            ((gone - r) / (gone - whole)).clamp(0.0, 1.0)
        }),
    };
    let width = usize::from(width);
    for (index, value) in coverage.iter_mut().enumerate() {
        let (column, row) = (index % width, index / width);
        let x = left + (column as f32 + 0.5) / width as f32 * w;
        let y = top + (row as f32 + 0.5) / f32::from(height) * h;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a product of a byte and a fraction"
        )]
        let faded = (f32::from(*value) * alpha(x, y)).round() as u8;
        *value = faded;
    }
}

/// Blurs `coverage`, a cell of `size` texels, by about a Gaussian of standard deviation `sigma`
/// texels: three box blurs across and three down, each box `2r + 1` wide where three passes'
/// variance, `((2r + 1)^2 - 1) / 4`, is `sigma` squared. Beyond the cell is clear.
fn blur(coverage: &mut [u8], (width, height): (u16, u16), sigma: f32) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few texels"
    )]
    let radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0)
        .round()
        .max(1.0) as usize;
    let (width, height) = (usize::from(width), usize::from(height));
    let mut values: Vec<f32> = coverage.iter().map(|&value| f32::from(value)).collect();
    let mut line = Vec::new();
    for _ in 0..3 {
        for row in 0..height {
            box_blur(&mut values, row * width, 1, width, radius, &mut line);
        }
        for column in 0..width {
            box_blur(&mut values, column, width, height, radius, &mut line);
        }
    }
    for (value, blurred) in coverage.iter_mut().zip(values) {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an average of bytes"
        )]
        let byte = blurred.round().clamp(0.0, 255.0) as u8;
        *value = byte;
    }
}

/// Replaces the `count` values of `values` from `start`, `stride` apart, with the average of
/// each one's box `2 radius + 1` wide, counting values beyond either end as clear.
fn box_blur(
    values: &mut [f32],
    start: usize,
    stride: usize,
    count: usize,
    radius: usize,
    line: &mut Vec<f32>,
) {
    line.clear();
    line.extend((0..count).map(|i| values[start + i * stride]));
    #[expect(clippy::cast_precision_loss, reason = "a few texels")]
    let span = (2 * radius + 1) as f32;
    let mut sum: f32 = line.iter().take(radius + 1).sum();
    for i in 0..count {
        values[start + i * stride] = sum / span;
        if let Some(entering) = line.get(i + radius + 1) {
            sum += entering;
        }
        if i >= radius {
            sum -= line[i - radius];
        }
    }
}

/// The cell of `cell` placed at `rect`, keyed by its icon and width.
#[must_use]
pub(crate) fn icon_cell(cell: Cell, rect: [u16; 4]) -> IconCell {
    IconCell {
        icon: orb::icon(orb::SIDES[cell.seat], cell.at),
        size: cell.width(),
        rect,
    }
}

#[cfg(test)]
mod tests {
    use super::{cells, rasterize};

    /// Every cell has ink, and none touches its cell's edge, where a cut would show a hard
    /// line. The bezel's bands are cut by their own fade, which is clear at a cut edge or
    /// hidden under the next band, so only their left and right edges are held.
    #[test]
    fn every_cell_has_ink_inside_its_edges() {
        for cell in cells() {
            let bitmap = rasterize(cell, 2.0);
            let (width, height) = (usize::from(bitmap.width), usize::from(bitmap.height));
            let at = |x: usize, y: usize| bitmap.coverage[y * width + x];
            assert!(
                bitmap.coverage.iter().any(|&value| value > 32),
                "{cell:?} has no ink"
            );
            let banded = matches!(
                beryl_scene::art::orb::LAYERS[cell.at].ramp,
                beryl_scene::art::orb::Ramp::Down(_)
            );
            let edge = (0..height).flat_map(|y| [at(0, y), at(width - 1, y)]);
            let edge: Vec<u8> = if banded {
                edge.collect()
            } else {
                edge.chain((0..width).flat_map(|x| [at(x, 0), at(x, height - 1)]))
                    .collect()
            };
            assert!(
                edge.iter().all(|&value| value < 16),
                "{cell:?} has ink at its edge: {:?}",
                edge.iter().max()
            );
        }
    }
}
