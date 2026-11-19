//! What an element's box is painted with besides one flat color, with CSS's meanings: the
//! gradients of `background-image`, the gradient of `mask-image`, and the color filters and blur
//! of `filter` (docs/beryl-3d.md, "Matching the web interface's look"). Each gradient resolves
//! against its element's box into the stop offsets and geometry the renderer draws
//! (`Gradient::resolve`), as CSS Images 3 places them.

use glam::Vec2;

/// A gradient length: logical points, or a percentage of a length the gradient gives it (the
/// gradient line's for a stop, the box's side for a radial gradient's size or centre).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Points(f32),
    Percent(f32),
}

impl Length {
    /// The length in points, a percentage taken of `of`.
    #[must_use]
    pub fn resolve(self, of: f32) -> f32 {
        match self {
            Self::Points(points) => points,
            Self::Percent(percent) => percent / 100.0 * of,
        }
    }
}

/// One color stop: its color, not premultiplied, and its position on the gradient line, or
/// `None` for CSS's default (the first at 0%, the last at 100%, and those between spread evenly
/// between their neighbours).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stop {
    pub color: [u8; 4],
    pub at: Option<Length>,
}

impl Stop {
    /// A stop with CSS's default position.
    #[must_use]
    pub const fn new(color: [u8; 4]) -> Self {
        Self { color, at: None }
    }

    /// A stop `percent` of the way along the gradient line.
    #[must_use]
    pub const fn percent(color: [u8; 4], percent: f32) -> Self {
        Self {
            color,
            at: Some(Length::Percent(percent)),
        }
    }

    /// A stop `points` along the gradient line.
    #[must_use]
    pub const fn points(color: [u8; 4], points: f32) -> Self {
        Self {
            color,
            at: Some(Length::Points(points)),
        }
    }
}

/// The most color stops a gradient draws; any after these are left out. The web interface's
/// primary button has eight.
pub const MOST_STOPS: usize = 8;

/// How large a radial gradient's ending shape is: CSS's `<radial-extent>` keywords, or explicit
/// radii (a circle reads only the first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extent {
    ClosestSide,
    ClosestCorner,
    FarthestSide,
    FarthestCorner,
    Size(Length, Length),
}

/// A gradient's geometry, as CSS writes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// `linear-gradient(<angle>, ...)`: the angle in degrees, 0 pointing up and 90 right.
    Linear { angle: f32 },
    /// `radial-gradient(<circle|ellipse> <extent> at <x> <y>, ...)`, the centre measured from
    /// the box's top left corner (a percentage of its width and its height).
    Radial {
        circle: bool,
        extent: Extent,
        at: (Length, Length),
    },
}

/// A CSS linear or radial gradient of up to `MOST_STOPS` stops. Not repeating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    pub shape: Shape,
    stops: [Stop; MOST_STOPS],
    count: u8,
}

/// Where a resolved gradient's line runs, in logical points of its box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geometry {
    /// The gradient line through the box's middle at `angle` radians (0 up, clockwise), whose
    /// length CSS gives as the box's extent along it.
    Linear { angle: f32 },
    /// The ending shape's centre from the box's top left corner and its two radii.
    Radial { centre: Vec2, radii: Vec2 },
}

/// A gradient resolved against a box: the line or shape it runs along and every stop's color
/// and offset along it, from 0 at the start to 1 at the ending shape. Stops past `count` repeat
/// the last one, so a shader may read all `MOST_STOPS`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolved {
    pub geometry: Geometry,
    pub colors: [[u8; 4]; MOST_STOPS],
    pub offsets: [f32; MOST_STOPS],
}

impl Gradient {
    /// A linear gradient at `angle` degrees through `stops`, of which the first `MOST_STOPS`
    /// are kept.
    #[must_use]
    pub const fn linear(angle: f32, stops: &[Stop]) -> Self {
        Self::of(Shape::Linear { angle }, stops)
    }

    /// A radial gradient through `stops`.
    #[must_use]
    pub const fn radial(
        circle: bool,
        extent: Extent,
        at: (Length, Length),
        stops: &[Stop],
    ) -> Self {
        Self::of(Shape::Radial { circle, extent, at }, stops)
    }

    /// A gradient of `shape` through `stops`.
    #[must_use]
    pub const fn of(shape: Shape, stops: &[Stop]) -> Self {
        let mut kept = [Stop {
            color: [0; 4],
            at: None,
        }; MOST_STOPS];
        let mut count = 0;
        while count < stops.len() && count < MOST_STOPS {
            kept[count] = stops[count];
            count += 1;
        }
        #[expect(clippy::cast_possible_truncation, reason = "at most MOST_STOPS")]
        let count = count as u8;
        Self {
            shape,
            stops: kept,
            count,
        }
    }

    /// Its stops.
    #[must_use]
    pub fn stops(&self) -> &[Stop] {
        &self.stops[..usize::from(self.count)]
    }

    /// The same gradient with each stop's color changed by `change`.
    #[must_use]
    pub fn map_colors(mut self, change: impl Fn([u8; 4]) -> [u8; 4]) -> Self {
        for stop in &mut self.stops[..usize::from(self.count)] {
            stop.color = change(stop.color);
        }
        self
    }

    /// The gradient as it is drawn on a box `size` logical points large.
    #[must_use]
    pub fn resolve(&self, size: Vec2) -> Resolved {
        let (geometry, line) = match self.shape {
            Shape::Linear { angle } => {
                let radians = angle.to_radians();
                // CSS Images 3: the gradient line's length is the box's extent along it.
                let line = (size.x * radians.sin()).abs() + (size.y * radians.cos()).abs();
                (Geometry::Linear { angle: radians }, line)
            }
            Shape::Radial { circle, extent, at } => {
                let centre = Vec2::new(at.0.resolve(size.x), at.1.resolve(size.y));
                let radii = radii(circle, extent, centre, size);
                // A radial gradient's line is its horizontal ray, as long as the ending shape's
                // horizontal radius.
                (Geometry::Radial { centre, radii }, radii.x)
            }
        };
        let stops = self.stops();
        let mut colors = [[0; 4]; MOST_STOPS];
        let mut offsets = [0.0; MOST_STOPS];
        for (at, stop) in stops.iter().enumerate() {
            colors[at] = stop.color;
            offsets[at] = match stop.at {
                Some(length) => length.resolve(line) / line.max(1e-6),
                None if at == 0 => 0.0,
                None if at + 1 == stops.len() => 1.0,
                None => f32::NAN,
            };
        }
        fix_offsets(&mut offsets[..stops.len()]);
        if let Some(last) = stops.len().checked_sub(1) {
            for at in stops.len()..MOST_STOPS {
                colors[at] = colors[last];
                offsets[at] = offsets[last];
            }
        }
        Resolved {
            geometry,
            colors,
            offsets,
        }
    }
}

/// Fixes the offsets of a stop list as CSS Images 3 does ("color stop fixup"): a stop placed
/// before an earlier one moves up to it, and a run of stops with no position is spread evenly
/// between the positioned stops either side. A missing offset is `NAN`; the first and the last
/// stop have theirs.
fn fix_offsets(offsets: &mut [f32]) {
    let mut most = f32::NEG_INFINITY;
    for offset in offsets.iter_mut().filter(|offset| !offset.is_nan()) {
        *offset = offset.max(most);
        most = *offset;
    }
    let mut at = 0;
    while at < offsets.len() {
        if !offsets[at].is_nan() {
            at += 1;
            continue;
        }
        let start = at - 1;
        let end = (at..offsets.len())
            .find(|&next| !offsets[next].is_nan())
            .unwrap_or(offsets.len() - 1);
        #[expect(clippy::cast_precision_loss, reason = "a handful of stops")]
        let steps = (end - start) as f32;
        let (from, to) = (offsets[start], offsets[end]);
        for (step, offset) in offsets[start + 1..end].iter_mut().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "a handful of stops")]
            let fraction = (step + 1) as f32 / steps;
            *offset = from + (to - from) * fraction;
        }
        at = end + 1;
    }
}

/// A radial gradient's two radii for its `extent` around `centre`, in a box `size` large
/// (CSS Images 3, "`radial-gradient()` syntax"). A corner keyword gives an ellipse the aspect
/// ratio its side keyword would and makes it pass through the corner.
fn radii(circle: bool, extent: Extent, centre: Vec2, size: Vec2) -> Vec2 {
    let near = Vec2::new(
        centre.x.min(size.x - centre.x),
        centre.y.min(size.y - centre.y),
    )
    .abs();
    let far = Vec2::new(
        centre.x.max(size.x - centre.x),
        centre.y.max(size.y - centre.y),
    )
    .abs();
    let radii = match (extent, circle) {
        (Extent::ClosestSide, true) => Vec2::splat(near.x.min(near.y)),
        (Extent::FarthestSide, true) => Vec2::splat(far.x.max(far.y)),
        (Extent::ClosestCorner, true) => Vec2::splat(near.length()),
        (Extent::FarthestCorner, true) => Vec2::splat(far.length()),
        (Extent::ClosestSide, false) => near,
        (Extent::FarthestSide, false) => far,
        (Extent::ClosestCorner, false) => near * std::f32::consts::SQRT_2,
        (Extent::FarthestCorner, false) => far * std::f32::consts::SQRT_2,
        (Extent::Size(x, _), true) => Vec2::splat(x.resolve(size.x)),
        (Extent::Size(x, y), false) => Vec2::new(x.resolve(size.x), y.resolve(size.y)),
    };
    radii.max(Vec2::splat(1e-3))
}

/// One function of CSS's `filter` other than `drop-shadow()`, which `Style::drop_shadow` holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    /// Multiplies each color channel: `brightness(1.1)` is 10% brighter.
    Brightness(f32),
    /// Moves each color toward its luminance by the amount, from 0 to 1.
    Grayscale(f32),
    /// Scales each color's distance from its luminance: under 1 is duller.
    Saturate(f32),
    /// Blurs the element with a Gaussian of CSS's length, the standard deviation in points.
    Blur(f32),
}

impl Filter {
    /// `color` changed by this function, as CSS's filter functions change a color: on its sRGB
    /// values, not premultiplied, clamped to 0 to 255. A blur changes no color.
    #[must_use]
    pub fn color(self, color: [u8; 4]) -> [u8; 4] {
        let [r, g, b, a] = color.map(f32::from);
        // Filter Effects 1's matrices, each row the weights of red, green and blue.
        let rows = match self {
            Self::Brightness(amount) => {
                [[amount, 0.0, 0.0], [0.0, amount, 0.0], [0.0, 0.0, amount]]
            }
            Self::Grayscale(amount) => {
                let keep = 1.0 - amount.clamp(0.0, 1.0);
                [
                    [
                        0.2126 + 0.7874 * keep,
                        0.7152 - 0.7152 * keep,
                        0.0722 - 0.0722 * keep,
                    ],
                    [
                        0.2126 - 0.2126 * keep,
                        0.7152 + 0.2848 * keep,
                        0.0722 - 0.0722 * keep,
                    ],
                    [
                        0.2126 - 0.2126 * keep,
                        0.7152 - 0.7152 * keep,
                        0.0722 + 0.9278 * keep,
                    ],
                ]
            }
            Self::Saturate(amount) => [
                [
                    0.213 + 0.787 * amount,
                    0.715 - 0.715 * amount,
                    0.072 - 0.072 * amount,
                ],
                [
                    0.213 - 0.213 * amount,
                    0.715 + 0.285 * amount,
                    0.072 - 0.072 * amount,
                ],
                [
                    0.213 - 0.213 * amount,
                    0.715 - 0.715 * amount,
                    0.072 + 0.928 * amount,
                ],
            ],
            Self::Blur(_) => return color,
        };
        let channel = |row: [f32; 3]| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0 to 255 first"
            )]
            let value = (row[0] * r + row[1] * g + row[2] * b)
                .round()
                .clamp(0.0, 255.0) as u8;
            value
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an alpha from a u8"
        )]
        let alpha = a as u8;
        [channel(rows[0]), channel(rows[1]), channel(rows[2]), alpha]
    }
}

/// `color` changed by each of `filters` in order.
#[must_use]
pub fn filtered(filters: &[Filter], color: [u8; 4]) -> [u8; 4] {
    filters
        .iter()
        .fold(color, |color, filter| filter.color(color))
}

/// The standard deviation in points of the blur among `filters`, 0 with none. Blurs compose, so
/// their variances add.
#[must_use]
pub fn blur_of(filters: &[Filter]) -> f32 {
    let variance: f32 = (filters.iter())
        .map(|filter| match filter {
            Filter::Blur(sigma) => sigma * sigma,
            _ => 0.0,
        })
        .sum();
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLD: [u8; 4] = [228, 162, 66, 255];

    #[test]
    fn stops_without_positions_spread_between_their_neighbours() {
        let gradient = Gradient::linear(
            180.0,
            &[
                Stop::new([0; 4]),
                Stop::percent(GOLD, 70.0),
                Stop::new(GOLD),
                Stop::new(GOLD),
            ],
        );
        let resolved = gradient.resolve(Vec2::new(100.0, 50.0));
        let offsets = resolved.offsets;
        assert!((offsets[1] - 0.7).abs() < 1e-6, "{offsets:?}");
        assert!((offsets[2] - 0.85).abs() < 1e-6, "{offsets:?}");
        assert!((offsets[3] - 1.0).abs() < 1e-6, "{offsets:?}");
        // The unused stops repeat the last.
        assert!((offsets[7] - 1.0).abs() < 1e-6 && resolved.colors[7] == GOLD);
    }

    #[test]
    fn a_stop_before_an_earlier_one_moves_up_to_it_and_points_resolve_on_the_line() {
        // `linear-gradient(to bottom, transparent, #000 40px)` on a 120 point tall box.
        let gradient = Gradient::linear(
            180.0,
            &[Stop::percent([0; 4], 50.0), Stop::points(GOLD, 40.0)],
        );
        let offsets = gradient.resolve(Vec2::new(200.0, 120.0)).offsets;
        assert!((offsets[0] - 0.5).abs() < 1e-6 && (offsets[1] - 0.5).abs() < 1e-6);
        let mask = Gradient::linear(180.0, &[Stop::new([0; 4]), Stop::points(GOLD, 40.0)]);
        let offsets = mask.resolve(Vec2::new(200.0, 120.0)).offsets;
        assert!((offsets[1] - 40.0 / 120.0).abs() < 1e-6, "{offsets:?}");
    }

    #[test]
    fn a_diagonal_line_is_the_box_extent_along_it() {
        let gradient = Gradient::linear(45.0, &[Stop::new([0; 4]), Stop::points(GOLD, 10.0)]);
        let resolved = gradient.resolve(Vec2::new(80.0, 80.0));
        let line = 80.0 * std::f32::consts::SQRT_2;
        assert!((resolved.offsets[1] - 10.0 / line).abs() < 1e-5);
    }

    #[test]
    fn a_farthest_corner_ellipse_passes_through_the_corner() {
        let gradient = Gradient::radial(
            false,
            Extent::FarthestCorner,
            (Length::Percent(50.0), Length::Percent(75.0)),
            &[Stop::new([0; 4]), Stop::new(GOLD)],
        );
        let Geometry::Radial { centre, radii } = gradient.resolve(Vec2::new(300.0, 160.0)).geometry
        else {
            panic!("a radial gradient resolves to a radial geometry");
        };
        assert!((centre - Vec2::new(150.0, 120.0)).length() < 1e-4);
        // The farthest corner is 150 across and 120 up; the ellipse keeps that aspect ratio.
        let corner = Vec2::new(150.0, 120.0) / radii;
        assert!((corner.length() - 1.0).abs() < 1e-5, "{radii}");
    }

    #[test]
    fn filters_change_colors_as_css_does() {
        assert_eq!(
            Filter::Brightness(1.1).color([100, 200, 250, 128]),
            [110, 220, 255, 128]
        );
        let gray = Filter::Grayscale(1.0).color([255, 0, 0, 255]);
        assert_eq!(gray, [54, 54, 54, 255]);
        assert_eq!(Filter::Saturate(1.0).color(GOLD), GOLD);
        let sigma = blur_of(&[
            Filter::Blur(3.0),
            Filter::Brightness(0.5),
            Filter::Blur(4.0),
        ]);
        assert!((sigma - 5.0).abs() < 1e-6);
    }
}
