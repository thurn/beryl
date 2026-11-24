//! Drawing on a face's pixels: rounded boxes filled with colors and gradients and edged with
//! lines, with `tiny_skia`, and set text rasterized with `swash`.

use std::collections::BTreeMap;

use kit_text::text::fonts::{ALL, Font};
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pixmap, Point,
    RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

use super::layout::{Box2, DISC_SCALE, SetGlyph, SetText};

/// An opaque color, 8 bits a channel.
pub(crate) type Rgb = [u8; 3];

/// A color and how opaque it is, from 0 to 1.
pub(crate) fn color(rgb: Rgb, alpha: f32) -> Color {
    Color::from_rgba8(
        rgb[0],
        rgb[1],
        rgb[2],
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// `a` mixed with `b`, `share` of `a`, as CSS's `color-mix(in srgb, a share, b)`.
pub(crate) fn mix(a: Rgb, share: f32, b: Rgb) -> Rgb {
    std::array::from_fn(|i| {
        (f32::from(a[i]) * share + f32::from(b[i]) * (1.0 - share)).round() as u8
    })
}

/// A color from CSS's `hsl()`: hue in degrees, saturation and lightness from 0 to 100.
pub(crate) fn hsl(hue: f32, saturation: f32, lightness: f32) -> Rgb {
    let (s, l) = (saturation / 100.0, lightness / 100.0);
    let a = s * l.min(1.0 - l);
    let channel = |n: f32| {
        let k = (n + hue / 30.0).rem_euclid(12.0);
        let value = l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [channel(0.0), channel(8.0), channel(4.0)]
}

/// A box with rounded corners, `radius` on both axes at each corner, clamped to half its sides.
pub(crate) fn rounded(area: Box2, radius: f32) -> Option<Path> {
    let r = radius.min(area.width / 2.0).min(area.height / 2.0).max(0.0);
    // How far a cubic's control point lies along the tangent for a quarter circle.
    let k = r * 0.552_284_8;
    let (left, top, right, bottom) = (area.x, area.y, area.right(), area.bottom());
    let mut path = PathBuilder::new();
    path.move_to(left + r, top);
    path.line_to(right - r, top);
    path.cubic_to(right - r + k, top, right, top + r - k, right, top + r);
    path.line_to(right, bottom - r);
    path.cubic_to(
        right,
        bottom - r + k,
        right - r + k,
        bottom,
        right - r,
        bottom,
    );
    path.line_to(left + r, bottom);
    path.cubic_to(left + r - k, bottom, left, bottom - r + k, left, bottom - r);
    path.line_to(left, top + r);
    path.cubic_to(left, top + r - k, left + r - k, top, left + r, top);
    path.close();
    path.finish()
}

/// A circle of `radius` around `center`.
fn circle(center: Point, radius: f32) -> Option<Path> {
    PathBuilder::from_circle(center.x, center.y, radius)
}

/// Gradient stops from `(offset, color, alpha)` triples.
pub(crate) fn stops(stops: &[(f32, Rgb, f32)]) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|&(offset, rgb, alpha)| GradientStop::new(offset, color(rgb, alpha)))
        .collect()
}

/// A gradient running from `from` to `to` through `stops`.
pub(crate) fn linear(from: Point, to: Point, stops: Vec<GradientStop>) -> Shader<'static> {
    LinearGradient::new(from, to, stops, SpreadMode::Pad, Transform::identity())
        .unwrap_or(Shader::SolidColor(Color::TRANSPARENT))
}

/// A gradient from top to bottom of `area`.
pub(crate) fn vertical(area: Box2, stops: Vec<GradientStop>) -> Shader<'static> {
    linear(
        Point::from_xy(area.x, area.y),
        Point::from_xy(area.x, area.bottom()),
        stops,
    )
}

/// A radial gradient around `center`, `radii` wide and tall, out to its last stop at the edge.
pub(crate) fn radial(
    center: Point,
    radii: (f32, f32),
    stops: Vec<GradientStop>,
) -> Shader<'static> {
    let (rx, ry) = (radii.0.max(0.01), radii.1.max(0.01));
    // A circle of radius 1 around the origin, scaled to the ellipse and moved to its center.
    let transform = Transform::from_row(rx, 0.0, 0.0, ry, center.x, center.y);
    RadialGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(0.0, 0.0),
        1.0,
        stops,
        SpreadMode::Pad,
        transform,
    )
    .unwrap_or(Shader::SolidColor(Color::TRANSPARENT))
}

/// A face being drawn, with the glyph rasterizer and the glyphs it has drawn so far.
pub(crate) struct Canvas {
    pub(crate) pixmap: Pixmap,
    scale: ScaleContext,
    /// Each glyph's coverage and placement, by font, glyph, size in hundredths of a pixel and
    /// weight: rules text repeats its glyphs.
    glyphs: BTreeMap<(Font, u16, u32, u32), Option<Mask>>,
}

#[derive(Clone)]
struct Mask {
    left: i32,
    top: i32,
    width: u32,
    coverage: Vec<u8>,
}

impl Canvas {
    pub(crate) fn new(pixmap: Pixmap) -> Self {
        Self {
            pixmap,
            scale: ScaleContext::new(),
            glyphs: BTreeMap::new(),
        }
    }

    /// Fills `path` with `shader`.
    pub(crate) fn fill(&mut self, path: Option<&Path>, shader: Shader<'_>) {
        if let Some(path) = path {
            let paint = Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            };
            self.pixmap
                .fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    /// Fills `area`, its corners rounded by `radius`, with `shader`.
    pub(crate) fn fill_box(&mut self, area: Box2, radius: f32, shader: Shader<'_>) {
        self.fill(rounded(area, radius).as_ref(), shader);
    }

    /// Draws a line `width` wide just inside the edge of `area`, its corners rounded by
    /// `radius`, as CSS's `inset 0 0 0 <width>` shadow or a border does.
    pub(crate) fn edge_inside(&mut self, area: Box2, radius: f32, width: f32, ink: Color) {
        let half = width / 2.0;
        self.edge(
            area.inset(half, half, half, half),
            (radius - half).max(0.0),
            width,
            ink,
        );
    }

    /// Draws a line `width` wide just outside the edge of `area`, as CSS's `0 0 0 <width>`
    /// shadow does.
    pub(crate) fn edge_outside(&mut self, area: Box2, radius: f32, width: f32, ink: Color) {
        let half = width / 2.0;
        self.edge(
            area.inset(-half, -half, -half, -half),
            radius + half,
            width,
            ink,
        );
    }

    fn edge(&mut self, area: Box2, radius: f32, width: f32, ink: Color) {
        let Some(path) = rounded(area, radius) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color(ink);
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    /// Draws `text` with its top left corner at `origin`.
    pub(crate) fn text(&mut self, text: &SetText, origin: (f32, f32)) {
        for glyph in &text.glyphs {
            self.glyph(glyph, origin);
        }
    }

    fn glyph(&mut self, glyph: &SetGlyph, origin: (f32, f32)) {
        let (x, y) = (origin.0 + glyph.x, origin.1 + glyph.y);
        let key = (
            glyph.font,
            glyph.glyph,
            (glyph.size * 100.0).round() as u32,
            glyph.ink.weight.round() as u32,
        );
        if !self.glyphs.contains_key(&key) {
            let mask = rasterize(&mut self.scale, glyph);
            self.glyphs.insert(key, mask);
        }
        let Some(Some(mask)) = self.glyphs.get(&key).cloned() else {
            return;
        };
        let left = x.round() as i32 + mask.left;
        let top = y.round() as i32 - mask.top;
        if let Some(disc) = glyph.ink.disc {
            // The disc is centered on the glyph's own outline, whatever the font's metrics.
            let height = mask.coverage.len() as u32 / mask.width.max(1);
            let center = Point::from_xy(
                left as f32 + mask.width as f32 / 2.0,
                top as f32 + height as f32 / 2.0,
            );
            let radius = glyph.size * DISC_SCALE / 2.0;
            if glyph.ink.ringed {
                let ring = circle(center, radius + glyph.size * 0.06);
                self.fill(
                    ring.as_ref(),
                    Shader::SolidColor(color([0x10, 0x16, 0x1c], 1.0)),
                );
            }
            self.fill(
                circle(center, radius).as_ref(),
                Shader::SolidColor(color(disc, 1.0)),
            );
        }
        self.blend(&mask, left, top, glyph.ink.color);
    }

    /// Inks `mask`'s coverage in `ink` with its top left corner at (`left`, `top`).
    fn blend(&mut self, mask: &Mask, left: i32, top: i32, ink: Rgb) {
        if mask.width == 0 {
            return;
        }
        let (width, height) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let pixels = self.pixmap.data_mut();
        for (i, &cover) in mask.coverage.iter().enumerate() {
            let x = left + (i as u32 % mask.width) as i32;
            let y = top + (i as u32 / mask.width) as i32;
            if cover == 0 || x < 0 || y < 0 || x >= width || y >= height {
                continue;
            }
            let at = ((y * width + x) * 4) as usize;
            let a = u32::from(cover);
            // Source over destination, both premultiplied.
            for (channel, value) in pixels[at..at + 3].iter_mut().zip(ink) {
                *channel = ((u32::from(value) * a + u32::from(*channel) * (255 - a)) / 255) as u8;
            }
            let alpha = &mut pixels[at + 3];
            *alpha = (a + u32::from(*alpha) * (255 - a) / 255) as u8;
        }
    }
}

/// The glyph's coverage, unhinted, with the variable font's weight axis set.
fn rasterize(context: &mut ScaleContext, glyph: &SetGlyph) -> Option<Mask> {
    // A weight is a whole number from 100 to 900, which fits a `u16` exactly.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let face = glyph.font.face(glyph.ink.weight as u16, false);
    let font = FontRef::from_index(ALL[usize::from(face)].data, 0)?;
    let mut scaler = context
        .builder(font)
        .size(glyph.size)
        .hint(false)
        .variations([("wght", glyph.ink.weight)])
        .build();
    let image = Render::new(&[Source::Outline])
        .format(Format::Alpha)
        .render(&mut scaler, glyph.glyph)?;
    let placement = image.placement;
    Some(Mask {
        left: placement.left,
        top: placement.top,
        width: placement.width,
        coverage: image.data,
    })
}
