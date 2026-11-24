//! Box and drop shadows and borders used in both styles and draw records.

/// One shadow of `box-shadow`, or of a `drop-shadow` filter, with CSS's meanings: offsets, blur
/// and spread in logical points. A drop shadow has no spread and is never inset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// How far the shadow is moved right and down from what casts it.
    pub x: f32,
    pub y: f32,
    /// CSS's blur length, which the renderer turns into the Gaussian's standard deviation as
    /// `WebKit` draws it (`Shadow::sigma` for a box shadow, `drop_sigma` for a drop shadow).
    pub blur: f32,
    /// How far the shadow's shape grows past the element's box, or shrinks for a negative one.
    pub spread: f32,
    /// Color and opacity, not premultiplied.
    pub color: [u8; 4],
    /// Whether it falls inside the element's padding box, darkening it from its edges, rather
    /// than outside its border box.
    pub inset: bool,
}

impl Shadow {
    /// How much narrower the Gaussian `WebKit` draws is than the one CSS names. A box shadow's
    /// blur length is two standard deviations by the CSS specification, and a `drop-shadow`'s is
    /// one; `WebKit` at scale 2 draws each about 0.85 times as wide, measured on outer box shadows of blurs 16 and 24 and drop shadows of blurs 3 and 8 (a
    /// Gaussian edge of that deviation fits each profile within a level of 255).
    pub const WEBKIT_DEVIATION: f32 = 0.85;

    /// A shadow `x` right and `y` down, blurred by the CSS length `blur`, in `color`.
    #[must_use]
    pub const fn new(x: f32, y: f32, blur: f32, color: [u8; 4]) -> Self {
        Self {
            x,
            y,
            blur,
            spread: 0.0,
            color,
            inset: false,
        }
    }

    /// The same shadow, grown by `spread` points past the box.
    #[must_use]
    pub const fn spread(mut self, spread: f32) -> Self {
        self.spread = spread;
        self
    }

    /// The same shadow, falling inside the box.
    #[must_use]
    pub const fn inset(mut self) -> Self {
        self.inset = true;
        self
    }

    /// The standard deviation of the Gaussian a box shadow is blurred with, in logical points.
    #[must_use]
    pub fn sigma(&self) -> f32 {
        self.blur.max(0.0) * 0.5 * Self::WEBKIT_DEVIATION
    }

    /// The standard deviation of the Gaussian a drop shadow is blurred with, in logical points.
    ///
    /// `WebKit` keeps a `drop-shadow()`'s blur as a whole number of CSS pixels, dropping the
    /// fraction: the conformance samples drew the same glow for blurs of 1 and 1.5 points, and
    /// for 2 and 2.5, so the current phase's glow of 1.5 points is drawn with a blur of 1. Its
    /// Gaussian is also narrower than `WEBKIT_DEVIATION` alone gives, by a variance of
    /// `DROP_VARIANCE`, which matters only for small blurs.
    #[must_use]
    pub fn drop_sigma(&self) -> f32 {
        let wide = self.blur.max(0.0).trunc() * Self::WEBKIT_DEVIATION;
        (wide * wide - Self::DROP_VARIANCE).max(0.0).sqrt()
    }

    /// The variance, in square logical points, by which `WebKit`'s Gaussian for a drop shadow is
    /// narrower than `WEBKIT_DEVIATION` times its whole blur. Simulating a white glow on an icon
    /// and fitting it to `WebKit`'s pictures at scale 2 gave deviations of 1.2, 3.2 and 5.0
    /// pixels for blurs of 1, 2 and 3 points, where 0.85 times the blur gives 1.7, 3.4 and 5.1:
    /// each is the wider Gaussian less about 1.45 square pixels, which is 0.36 square points.
    pub const DROP_VARIANCE: f32 = 0.36;
}

/// One blurred copy of an element's text or icon that a chain of drop shadows draws under it
/// (`DropLayers`): moved `x` right and `y` down, blurred with standard deviation `sigma` logical
/// points, in `color`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DropLayer {
    pub x: f32,
    pub y: f32,
    pub sigma: f32,
    pub color: [u8; 4],
}

/// The most drop shadows of one element that are drawn; any after these are left out. Four
/// shadows make fifteen layers.
pub const MOST_DROP_SHADOWS: usize = 4;

/// The layers a chain of CSS drop shadows draws under an element's text or icon, bottom first.
///
/// CSS applies each `drop-shadow()` of a `filter` to the result of the ones before it, so the
/// second shadow is cast by the text and by the first shadow too. Blurs compose (two Gaussians
/// make one whose variance is the sum of theirs) and the shadows are faint where they overlap
/// the text, so the shadow of a shadow is drawn as a layer of its own: shadow `k` adds one layer
/// for the text and one for each layer the shadows before it made, with their offsets added,
/// their variances added and their opacities multiplied. A chain of `n` shadows makes `2^n - 1`
/// layers, and each shadow's layers are drawn under those of the shadows before it.
#[derive(Clone, Copy, Debug)]
pub struct DropLayers {
    layers: [DropLayer; (1 << MOST_DROP_SHADOWS) - 1],
    count: usize,
}

impl DropLayers {
    #[must_use]
    pub fn new(shadows: &[Shadow]) -> Self {
        let mut out = Self {
            layers: [DropLayer::default(); (1 << MOST_DROP_SHADOWS) - 1],
            count: 0,
        };
        // Each layer's variance, so that composing them adds variances, not deviations.
        let mut variances = [0.0_f32; (1 << MOST_DROP_SHADOWS) - 1];
        for shadow in shadows.iter().take(MOST_DROP_SHADOWS) {
            let sigma = shadow.drop_sigma();
            let before = out.count;
            // The text itself, then every layer the shadows before this one made.
            for from in 0..=before {
                let (base, variance) = match from.checked_sub(1) {
                    None => (
                        DropLayer {
                            color: [0, 0, 0, 255],
                            ..DropLayer::default()
                        },
                        0.0,
                    ),
                    Some(at) => (out.layers[at], variances[at]),
                };
                let variance = variance + sigma * sigma;
                let [red, green, blue, alpha] = shadow.color;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a product of two alphas is in 0 to 255"
                )]
                let alpha = (f32::from(alpha) * f32::from(base.color[3]) / 255.0).round() as u8;
                variances[out.count] = variance;
                out.layers[out.count] = DropLayer {
                    x: base.x + shadow.x,
                    y: base.y + shadow.y,
                    sigma: variance.sqrt(),
                    color: [red, green, blue, alpha],
                };
                out.count += 1;
            }
        }
        out
    }

    /// The layers, bottom first: the last shadow's first.
    pub fn iter(&self) -> impl Iterator<Item = &DropLayer> {
        self.layers[..self.count].iter().rev()
    }
}

/// The color and width of each side of an element's border, top, right, bottom and left, as
/// CSS's `border-color` and `border-width` list them. The border takes room inside the box as
/// CSS's does. Where two sides of different colors meet, the corner is split along the line from
/// the border box's corner to the padding box's, as CSS's border joins are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub color: [[u8; 4]; 4],
    pub width: [f32; 4],
}

impl Border {
    /// The same color and width on every side.
    #[must_use]
    pub const fn uniform(color: [u8; 4], width: f32) -> Self {
        Self {
            color: [color; 4],
            width: [width; 4],
        }
    }

    /// The same border with side `side` (0 top, 1 right, 2 bottom, 3 left) in `color`.
    #[must_use]
    pub const fn side_color(mut self, side: usize, color: [u8; 4]) -> Self {
        self.color[side] = color;
        self
    }

    /// The same border with side `side` (0 top, 1 right, 2 bottom, 3 left) `width` wide.
    #[must_use]
    pub const fn side_width(mut self, side: usize, width: f32) -> Self {
        self.width[side] = width;
        self
    }

    /// Whether every side has the same color and width.
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.color.iter().all(|color| *color == self.color[0])
            && (self.width.iter()).all(|width| width.to_bits() == self.width[0].to_bits())
    }

    /// The same border with every color changed by `change`.
    #[must_use]
    pub fn map_colors(mut self, change: impl Fn([u8; 4]) -> [u8; 4]) -> Self {
        self.color = self.color.map(change);
        self
    }
}

impl From<([u8; 4], f32)> for Border {
    fn from((color, width): ([u8; 4], f32)) -> Self {
        Self::uniform(color, width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_of_drop_shadows_shadows_the_shadows_before_it() {
        let first = Shadow::new(0.0, 1.0, 1.0, [255, 0, 0, 255]);
        let second = Shadow::new(0.0, 2.0, 2.0, [0, 0, 255, 128]);
        let layers: Vec<DropLayer> = DropLayers::new(&[first, second]).iter().copied().collect();
        // Bottom first: the second shadow of the first shadow, then of the text, then the first.
        assert_eq!(layers.len(), 3);
        let of_first = layers[0];
        assert!(of_first.x.abs() < 1e-6 && (of_first.y - 3.0).abs() < 1e-6);
        let expected = (first.drop_sigma().powi(2) + second.drop_sigma().powi(2)).sqrt();
        assert!((of_first.sigma - expected).abs() < 1e-6);
        assert_eq!(of_first.color, [0, 0, 255, 128]);
        assert!((layers[1].y - 2.0).abs() < 1e-6);
        assert_eq!(layers[2].color, [255, 0, 0, 255]);
    }
}
