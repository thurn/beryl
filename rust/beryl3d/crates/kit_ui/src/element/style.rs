//! An element's style, with CSS's names, and the builder methods that set it. Sizes are in
//! logical points. A field whose doc comment names a track is not laid out or drawn yet: the
//! element is laid out as if it had its default, until that track builds it.

use super::paint::{Filter, Gradient};
use super::{Element, Presence, Timing, Transition};
use glam::Vec2;
use kit_base::tokens::type_style;
use kit_text::text::TypeStyle;

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

/// The box that positions and clips one gradient, as one box keyword in CSS's `background`
/// shorthand sets both `background-origin` and `background-clip`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackgroundBox {
    #[default]
    Border,
    Padding,
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
    /// They are cut off, and the element can scroll on either axis.
    Scroll,
    /// They are cut off, and the element scrolls horizontally.
    ScrollX,
    /// They are cut off, and the element scrolls vertically.
    ScrollY,
}

use kit_base::paint::{Border, Shadow};

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
    /// How opaque the element and its children are, from 0 to 1.
    pub opacity: f32,
    /// A visual offset in logical points. It moves the element and its contents.
    pub translation: Vec2,
    /// Uniform visual scale about the element's centre.
    pub scale: f32,
    /// How changes to individual properties are drawn. The first entry for a property wins.
    pub transition: &'static [Transition],
    /// The screen's default fade when no opacity transition is declared.
    pub opacity_transition: Option<Timing>,
    /// How this element appears after it joins the tree.
    pub enter: Option<Presence>,
    /// How this element departs after it leaves the tree.
    pub leave: Option<Presence>,
    /// How a change to the element's layout position is drawn, without moving its children.
    pub layout_slide: Option<Timing>,
    /// The color of the element's panel in place of the usual one. An element that is not
    /// usually drawn on a panel is drawn on one of this color.
    pub background: Option<[u8; 4]>,
    /// Its gradients, CSS's `background-image`, the first drawn on top, over its `background`
    /// color and under its border. An element with gradients is drawn on a panel, as with a
    /// `background`.
    pub background_image: Vec<Gradient>,
    /// The positioning and clipping boxes of `background_image`, in the same order. A shorter
    /// list repeats as in CSS. An empty list draws every gradient across the border box.
    pub background_boxes: Vec<BackgroundBox>,
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
    /// The colors and widths of the element's border.
    pub border: Option<Border>,
    /// The radius of the element's panel's corners, or 0 for the usual radius: a screen's
    /// panel's or a button's, and square on any other element.
    pub radius: f32,
    /// Whether its first child is its legend, as a `<fieldset>`'s `<legend>` is: its panel and
    /// border start at the legend's vertical middle rather than at its box's top, and the top
    /// line of its border is left open across the legend's box, as browsers draw a fieldset.
    /// The element's box, and so its layout, is unchanged.
    pub legend: bool,
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
    /// CSS backdrop-filter functions applied to what is visible through this element.
    pub backdrop_filter: Box<[Filter]>,
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
            translation: Vec2::ZERO,
            scale: 1.0,
            transition: &[],
            opacity_transition: None,
            enter: None,
            leave: None,
            layout_slide: None,
            background: None,
            background_image: Vec::new(),
            background_boxes: Vec::new(),
            mask: None,
            scroll_fade: None,
            filter: Vec::new(),
            border: None,
            radius: 0.0,
            legend: false,
            box_shadow: Vec::new(),
            drop_shadow: Vec::new(),
            text: type_style::BODY,
            text_align: TextAlign::Start,
            color: None,
            fit: false,
            passes: false,
            backdrop: false,
            backdrop_filter: Box::default(),
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
    translation: Vec2,
    scale: f32,
    transition: &'static [Transition],
    radius: f32,
    legend: bool,
    box_shadow: Vec<Shadow>,
    drop_shadow: Vec<Shadow>,
    background_image: Vec<Gradient>,
    background_boxes: Vec<BackgroundBox>,
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
    enter: Presence,
    leave: Presence,
    layout_slide: Timing,
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
    color: [u8; 4],
    mask: Gradient,
    scroll_fade: f32,
}

impl Element {
    /// Filters the picture behind the element's box.
    #[must_use]
    pub fn backdrop_filter(mut self, filters: impl Into<Box<[Filter]>>) -> Self {
        self.style.backdrop_filter = filters.into();
        self
    }

    /// The element's border: a `Border`, or a color and a width for every side.
    #[must_use]
    pub fn border(mut self, border: impl Into<Border>) -> Self {
        self.style.border = Some(border.into());
        self
    }

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
