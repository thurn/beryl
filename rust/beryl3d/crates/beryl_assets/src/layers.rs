//! Rasterizes the outlines, coverage ramps and blurs of layered icons.

use kit_base::atlas::artwork::{CoverageLayer, CoverageRamp};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::glyphs::Bitmap;

/// Draws `layer` at `scale` physical pixels per point into coverage.
#[must_use]
pub(crate) fn rasterize(layer: &CoverageLayer, scale: f32) -> Bitmap {
    let [width, height] = layer.cell.texels(scale);
    let mut pixmap = Pixmap::new(u32::from(width), u32::from(height)).expect("a cell has an area");
    let [left, top, w, h] = layer.cell.view;
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
    for (points, closed) in &layer.outlines {
        let mut points = points.iter();
        let Some(first) = points.next() else {
            continue;
        };
        builder.move_to(first.x, first.y);
        for point in points {
            builder.line_to(point.x, point.y);
        }
        if *closed {
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
    fade(&mut coverage, layer, (width, height));
    // The blur's standard deviation in texels.
    let sigma = layer.blur * across;
    if sigma > 0.0 {
        if let Some(opacity) = layer.glow_opacity {
            let mut softened = coverage.clone();
            blur(&mut softened, (width, height), sigma);
            for (stroke, glow) in coverage.iter_mut().zip(softened) {
                let behind = f32::from(glow) * opacity * (1.0 - f32::from(*stroke) / 255.0);
                *stroke = stroke.saturating_add(behind.round() as u8);
            }
        } else {
            blur(&mut coverage, (width, height), sigma);
        }
    }
    Bitmap {
        width,
        height,
        bearing: [0, 0],
        coverage,
    }
}

/// Multiplies `coverage`, a cell of `size` texels, by the layer's ramp at each texel's
/// centre.
fn fade(coverage: &mut [u8], layer: &CoverageLayer, (width, height): (u16, u16)) {
    let [left, top, w, h] = layer.cell.view;
    let alpha: Box<dyn Fn(f32, f32) -> f32> = match layer.ramp {
        CoverageRamp::None => return,
        CoverageRamp::Vertical {
            top,
            height,
            stops: [rise, whole, keep, gone],
        } => Box::new(move |_, y| {
            let at = (y - top) / height;
            if at < rise || at > gone {
                0.0
            } else if at < whole {
                (at - rise) / (whole - rise)
            } else if at > keep {
                (gone - at) / (gone - keep)
            } else {
                1.0
            }
        }),
        CoverageRamp::Radial { whole, gone } => Box::new(move |x, y| {
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
