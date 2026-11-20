//! An element's style, with CSS's names, and the builder methods that set it. Sizes are in
//! logical points. A field whose doc comment names a track is not laid out or drawn yet: the
//! element is laid out as if it had its default, until that track builds it.

use super::Element;
use super::paint::{Filter, Gradient};
use crate::text::TypeStyle;
use crate::tokens::type_style;

/// Which way an element lays out its children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Row,
    Column,
}

/// How children are placed across the direction they are laid out in: CSS `align-items`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

/// How children are placed along the direction they are laid out in: CSS `justify-content`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
}

/// Where a text's lines sit across its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
}

/// How an element lays out its children: CSS `display`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Flex,
    Grid,
    /// The element and its children take no room and are not drawn.
    None,
}

/// Whether an element is placed by its parent's layout or at offsets from its parent's box:
/// CSS `position`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Relative,
    Absolute,
}

/// One column or row of a grid: CSS `grid-template-columns` and `grid-template-rows`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    /// A fixed size in logical points.
    Points(f32),
    /// A share of the room the fixed tracks leave: CSS `fr`.
    Fraction(f32),
    /// As large as its content.
    Auto,
}

/// What happens to children that do not fit inside an element: CSS `overflow`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overflow {
    #[default]
    Visible,
    /// They are cut off at the element's edges.
    Clip,
    /// They are cut off, and the element scrolls to show them.
    Scroll,
}

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
    /// one; `WebKit` at scale 2 draws each about 0.85 times as wide, which `just b3d match samples`
    /// measured on outer box shadows of blurs 16 and 24 and drop shadows of blurs 3 and 8 (a
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

/// An element's layout and look, with CSS's names.
#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a CSS property of its own, set and read apart from the others"
)]
pub struct Style {
    pub display: Display,
    pub direction: Direction,
    pub gap: f32,
    /// Top, right, bottom and left.
    pub padding: [f32; 4],
    /// Top, right, bottom and left.
    pub margin: [f32; 4],
    pub grow: f32,
    pub shrink: f32,
    /// The size along the parent's direction the element starts from before it grows or
    /// shrinks: CSS `flex-basis`.
    pub basis: Option<f32>,
    /// Whether children wrap onto more lines.
    pub wrap: bool,
    pub align_items: Option<Align>,
    /// How the element is placed across its parent's direction, in place of the parent's
    /// `align_items`.
    pub align_self: Option<Align>,
    pub justify_content: Option<Justify>,
    pub position: Position,
    /// Top, right, bottom and left offsets from the parent's box, for `Position::Absolute`.
    pub inset: [Option<f32>; 4],
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    pub grid_template_columns: Vec<Track>,
    pub grid_template_rows: Vec<Track>,
    /// The first and the last grid line the element spans, counting from 1 as CSS does.
    pub grid_column: Option<(i16, i16)>,
    /// As `grid_column`, for rows.
    pub grid_row: Option<(i16, i16)>,
    pub overflow: Overflow,
    /// How opaque the element and its children are, from 0 to 1. A change of it on a kept
    /// element of the screen fades over a transition (`layout::motion`).
    pub opacity: f32,
    /// The color of the element's panel in place of the usual one. An element that is not
    /// usually drawn on a panel is drawn on one of this color.
    pub background: Option<[u8; 4]>,
    /// Its gradients, CSS's `background-image`, the first drawn on top, over its `background`
    /// color and under its border. An element with gradients is drawn on a panel, as with a
    /// `background`.
    pub background_image: Vec<Gradient>,
    /// A gradient whose opacity, at each point of its box, scales the opacity of the element and
    /// everything under it there, as CSS's `mask-image`; outside its box they are cut off. Only a
    /// linear gradient is drawn.
    pub mask: Option<Gradient>,
    /// How far, in points, a scrolling element's content fades out at each end that has more
    /// beyond it, as the web's pile viewer masks its list (`.zone-viewer-cards[data-more]` in
    /// `ui/src/screens/styles/zone-viewer.css`). The flat layer draws it as a mask in place of
    /// `mask`, chosen by where the element is scrolled.
    pub scroll_fade: Option<f32>,
    /// The functions of CSS's `filter` other than `drop-shadow()`, in order. The color filters
    /// change every color the element and everything under it is drawn in; a blur softens the
    /// edges of its own panel and gradients.
    pub filter: Vec<Filter>,
    /// The color and width of the element's border, which takes room inside its box as CSS's
    /// does.
    pub border: Option<([u8; 4], f32)>,
    /// The radius of the element's panel's corners, or 0 for the usual radius: a screen's
    /// panel's or a button's, and square on any other element.
    pub radius: f32,
    /// Its box shadows, CSS's `box-shadow`, the first drawn on top. An outer shadow is drawn
    /// under the element's panel and only outside its border box; an inset one over the panel's
    /// fill, inside its padding box. An element with shadows and no panel still casts them.
    pub box_shadow: Vec<Shadow>,
    /// The shadows of its text and icon, CSS's `filter: drop-shadow(...)`, each a blurred copy
    /// of the glyphs' and icon's coverage drawn under them in the shadow's color.
    pub drop_shadow: Vec<Shadow>,
    /// How the element's own text looks.
    pub text: TypeStyle,
    pub text_align: TextAlign,
    /// The color of the element's text in place of the design tokens' type color.
    pub color: Option<[u8; 4]>,
    /// Whether its text stays on one line and shrinks until it fits the content box, as the
    /// web interface's `useTextFit` (`ui/src/components/fit.ts`) shrinks a button's long label,
    /// rather than breaking onto more lines (`layout::fit`).
    pub fit: bool,
    /// Whether the pointer goes through it and everything under it to what lies beneath, as
    /// CSS's `pointer-events: none` lets it: a press never lands on it, and what it covers
    /// stays hovered. Its resolved elements have the `PASSES` state.
    pub passes: bool,
    /// Whether its `filter` is CSS's `backdrop-filter` instead: a blur and color filters applied
    /// to the whole window as drawn before the element, the table and every flat-layer element
    /// under it, and not to the element itself. The flat layer writes an anchored child of the
    /// screen with one after its floats, so that they are filtered too, and only the first such
    /// element of a frame filters anything (`FrameBuilder::backdrop`). The web's end screen
    /// blurs the table this way, though its CSS puts the `filter` on the table
    /// (`.table-dimmed`). A flag rather than a list of its own keeps `Style` from growing,
    /// which added reallocations to every board's `Client::handle`.
    pub backdrop: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Flex,
            direction: Direction::Row,
            gap: 0.0,
            padding: [0.0; 4],
            margin: [0.0; 4],
            grow: 0.0,
            shrink: 1.0,
            basis: None,
            wrap: false,
            align_items: None,
            align_self: None,
            justify_content: None,
            position: Position::Relative,
            inset: [None; 4],
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_column: None,
            grid_row: None,
            overflow: Overflow::Visible,
            opacity: 1.0,
            background: None,
            background_image: Vec::new(),
            mask: None,
            scroll_fade: None,
            filter: Vec::new(),
            border: None,
            radius: 0.0,
            box_shadow: Vec::new(),
            drop_shadow: Vec::new(),
            text: type_style::BODY,
            text_align: TextAlign::Start,
            color: None,
            fit: false,
            passes: false,
            backdrop: false,
        }
    }
}

/// Builder methods that set one field of `Style` each, named for the field.
macro_rules! setters {
    ($($name:ident: $type:ty),* $(,)?) => {
        impl Element {
            $(
                #[must_use]
                pub fn $name(mut self, $name: $type) -> Self {
                    self.style.$name = $name;
                    self
                }
            )*
        }
    };
}

setters! {
    display: Display,
    gap: f32,
    margin: [f32; 4],
    grow: f32,
    shrink: f32,
    wrap: bool,
    position: Position,
    inset: [Option<f32>; 4],
    grid_template_columns: Vec<Track>,
    grid_template_rows: Vec<Track>,
    overflow: Overflow,
    opacity: f32,
    radius: f32,
    box_shadow: Vec<Shadow>,
    drop_shadow: Vec<Shadow>,
    background_image: Vec<Gradient>,
    filter: Vec<Filter>,
    fit: bool,
    passes: bool,
}

/// Builder methods that set one optional field of `Style` each to `Some`, named for the field.
macro_rules! optional_setters {
    ($($name:ident: $type:ty),* $(,)?) => {
        impl Element {
            $(
                #[must_use]
                pub fn $name(mut self, $name: $type) -> Self {
                    self.style.$name = Some($name);
                    self
                }
            )*
        }
    };
}

optional_setters! {
    basis: f32,
    align_items: Align,
    align_self: Align,
    justify_content: Justify,
    width: f32,
    height: f32,
    min_width: f32,
    max_width: f32,
    min_height: f32,
    max_height: f32,
    grid_column: (i16, i16),
    grid_row: (i16, i16),
    background: [u8; 4],
    border: ([u8; 4], f32),
    color: [u8; 4],
    mask: Gradient,
    scroll_fade: f32,
}

impl Element {
    /// The same padding on every side.
    #[must_use]
    pub fn padding(mut self, padding: f32) -> Self {
        self.style.padding = [padding; 4];
        self
    }

    /// How the element's text looks.
    #[must_use]
    pub fn style(mut self, style: TypeStyle) -> Self {
        self.style.text = style;
        self
    }

    #[must_use]
    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.style.text_align = align;
        self
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
