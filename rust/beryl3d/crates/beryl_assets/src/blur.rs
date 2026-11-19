//! Blurring a glyph's or an icon's coverage for a drop shadow (docs/beryl-3d.md, "Matching the
//! web interface's look"). The Baseline shader may take one texture sample a fragment, so a drop
//! shadow cannot blur as it draws: the glyph worker rasterizes a blurred copy of the coverage
//! into a cell of its own, and the client draws that cell with the glyph or icon material in the
//! shadow's color (`crate::icons::rasterize_shadows` for an icon's).

use crate::glyphs::Bitmap;

/// `bitmap` blurred by a Gaussian of standard deviation `sigma` physical pixels, on a cell grown
/// by `reach(sigma)` texels on every side, with its bearing moved to match. The blur is
/// separable, so it runs across the rows and then down the columns.
#[must_use]
pub(crate) fn blurred(bitmap: &Bitmap, sigma: f32) -> Bitmap {
    let pad = reach(sigma);
    let (width, height) = (usize::from(bitmap.width), usize::from(bitmap.height));
    let (wide, tall) = (width + 2 * pad, height + 2 * pad);
    // The coverage on the grown cell, from 0 to 1.
    let mut grown = vec![0.0_f32; wide * tall];
    for y in 0..height {
        for x in 0..width {
            grown[(y + pad) * wide + x + pad] = f32::from(bitmap.coverage[y * width + x]) / 255.0;
        }
    }
    let coverage = blur_plane(&grown, wide, sigma).iter().map(|&value| level(value)).collect();
    let shift = i16::try_from(pad).unwrap_or(i16::MAX);
    Bitmap {
        width: u16::try_from(wide).unwrap_or(u16::MAX),
        height: u16::try_from(tall).unwrap_or(u16::MAX),
        bearing: [
            bitmap.bearing[0].saturating_sub(shift),
            bitmap.bearing[1].saturating_sub(shift),
        ],
        coverage,
    }
}

/// `plane`, rows of `wide` values, blurred by a Gaussian of standard deviation `sigma` texels,
/// with nothing past its edges. The blur is separable, so it runs across the rows and then down
/// the columns.
#[must_use]
pub(crate) fn blur_plane(plane: &[f32], wide: usize, sigma: f32) -> Vec<f32> {
    let tall = plane.len() / wide.max(1);
    let pad = reach(sigma);
    let kernel = kernel(sigma, pad);
    let mut across = vec![0.0_f32; wide * tall];
    for y in 0..tall {
        for x in 0..wide {
            across[y * wide + x] = taps(&kernel, pad, x, wide, |at| plane[y * wide + at]);
        }
    }
    let mut out = vec![0.0_f32; wide * tall];
    for y in 0..tall {
        for x in 0..wide {
            out[y * wide + x] = taps(&kernel, pad, y, tall, |at| across[at * wide + x]);
        }
    }
    out
}

/// A coverage from 0 to 1 as a level from 0 to 255.
#[must_use]
pub(crate) fn level(value: f32) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0 to 255 first"
    )]
    let level = (value * 255.0).round().clamp(0.0, 255.0) as u8;
    level
}

/// How many texels a blur of `sigma` reaches past its shape: three deviations, past which the
/// Gaussian leaves under a level of 255.
#[must_use]
pub(crate) fn reach(sigma: f32) -> usize {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small positive number"
    )]
    let texels = (3.0 * sigma.max(0.0)).ceil() as usize;
    texels
}

/// The Gaussian's weights from `-pad` to `pad` texels, summing to 1.
fn kernel(sigma: f32, pad: usize) -> Vec<f32> {
    let sigma = sigma.max(1e-3);
    #[expect(clippy::cast_precision_loss, reason = "a small offset")]
    let weights: Vec<f32> = (0..=2 * pad)
        .map(|i| {
            let x = i as f32 - pad as f32;
            (-(x * x) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let total: f32 = weights.iter().sum();
    weights.into_iter().map(|w| w / total).collect()
}

/// The kernel's weighted sum of `read` around `at`, along a line `length` long, with nothing
/// past its ends.
fn taps(kernel: &[f32], pad: usize, at: usize, length: usize, read: impl Fn(usize) -> f32) -> f32 {
    let mut sum = 0.0;
    for (i, weight) in kernel.iter().enumerate() {
        let Some(from) = (at + i).checked_sub(pad) else {
            continue;
        };
        if from < length {
            sum += weight * read(from);
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blur_keeps_the_coverage_and_grows_the_cell() {
        let bitmap = Bitmap {
            width: 4,
            height: 4,
            bearing: [1, -5],
            coverage: vec![255; 16],
        };
        let found = blurred(&bitmap, 2.0);
        assert_eq!((found.width, found.height), (16, 16));
        assert_eq!(found.bearing, [-5, -11]);
        let total: u32 = found.coverage.iter().map(|&c| u32::from(c)).sum();
        // The same ink, spread out: within rounding of 16 full texels.
        assert!((i64::from(total) - 16 * 255).abs() < 128, "{total}");
    }

}
