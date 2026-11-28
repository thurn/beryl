//! `--region` and `--beside`, which `just client bug` passes to draw a bug report's moment the
//! way its screenshot shows it: `--region` cuts the picture to the box the player selected, in
//! logical points, and `--beside` puts another picture, the report's own screenshot, on the
//! left of the drawn one, so that the two can be compared in one image.

use std::path::Path;

use beryl_inspect::cli::Usage;

use super::{Error, Region};

/// The black gap between the two pictures, in pixels.
const GAP: u32 = 16;

/// The pixels of the box `--region <x>,<y>,<width>,<height>` names in logical points, in a
/// picture of `size` pixels at `scale`, cut to the picture's edges.
pub(crate) fn region(text: &str, scale: f32, size: (u32, u32)) -> Result<Region, Usage> {
    let values: Vec<f32> = text
        .split(',')
        .map(|word| word.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    let &[x, y, width, height] = values.as_slice() else {
        return Err(Usage(format!(
            "--region {text}: write <x>,<y>,<width>,<height> in logical points"
        )));
    };
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
        || width <= 0.0
        || height <= 0.0
    {
        return Err(Usage(format!(
            "--region {text}: the box needs a positive width and height and no negative value"
        )));
    }
    let pixels = |value: f32| (value * scale).round() as u32;
    let left = pixels(x).min(size.0.saturating_sub(1));
    let top = pixels(y).min(size.1.saturating_sub(1));
    Ok(Region {
        x: left,
        y: top,
        width: pixels(width).clamp(1, size.0 - left),
        height: pixels(height).clamp(1, size.1 - top),
    })
}

/// `drawn`, `width` by `height` RGBA pixels, with the PNG at `path` on its left, scaled to the
/// same height, and a black gap between them.
pub(crate) fn beside(
    path: &Path,
    drawn: &[u8],
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, u32, u32), Error> {
    let other = beryl_render::read_rgba_png(path).map_err(Error::Failed)?;
    if other.width == 0 || other.height == 0 {
        return Err(Error::Failed(format!(
            "{}: the picture is empty",
            path.display()
        )));
    }
    let other_width =
        ((u64::from(other.width) * u64::from(height)) / u64::from(other.height)).max(1) as u32;
    let total = other_width + GAP + width;
    let mut out = vec![0u8; (total * height * 4) as usize];
    for opacity in out.iter_mut().skip(3).step_by(4) {
        *opacity = 255;
    }
    for row in 0..height {
        // Nearest-neighbour scaling, which keeps the screenshot's text sharp when the two are
        // already the same size, as a report's capture usually is.
        let from_row = (u64::from(row) * u64::from(other.height) / u64::from(height)) as u32;
        for column in 0..other_width {
            let from_column =
                (u64::from(column) * u64::from(other.width) / u64::from(other_width)) as u32;
            let from = ((from_row * other.width + from_column) * 4) as usize;
            let to = ((row * total + column) * 4) as usize;
            out[to..to + 4].copy_from_slice(&other.pixels[from..from + 4]);
        }
        let to = ((row * total + other_width + GAP) * 4) as usize;
        let from = (row * width * 4) as usize;
        out[to..to + (width * 4) as usize]
            .copy_from_slice(&drawn[from..from + (width * 4) as usize]);
    }
    Ok((out, total, height))
}
