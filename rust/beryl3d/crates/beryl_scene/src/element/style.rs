//! An element's style, with CSS's names, and the builder methods that set it. Sizes are in
//! logical points. A field whose doc comment names a track is not laid out or drawn yet: the
//! element is laid out as if it had its default, until that track builds it.

use super::Element;
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

/// An element's layout and look, with CSS's names.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// Grid and `None`: the elements track.
    pub display: Display,
    pub direction: Direction,
    pub gap: f32,
    /// Top, right, bottom and left.
    pub padding: [f32; 4],
    /// Top, right, bottom and left: the elements track.
    pub margin: [f32; 4],
    pub grow: f32,
    /// The elements track.
    pub shrink: f32,
    /// The elements track.
    pub basis: Option<f32>,
    /// Whether children wrap onto more lines: the elements track.
    pub wrap: bool,
    pub align_items: Option<Align>,
    /// The elements track.
    pub align_self: Option<Align>,
    pub justify_content: Option<Justify>,
    /// The elements track.
    pub position: Position,
    /// Top, right, bottom and left offsets from the parent's box, for `Position::Absolute`:
    /// the elements track.
    pub inset: [Option<f32>; 4],
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// The elements track.
    pub min_width: Option<f32>,
    /// The elements track.
    pub max_width: Option<f32>,
    /// The elements track.
    pub min_height: Option<f32>,
    /// The elements track.
    pub max_height: Option<f32>,
    /// The elements track.
    pub grid_template_columns: Vec<Track>,
    /// The elements track.
    pub grid_template_rows: Vec<Track>,
    /// The first and the last grid line the element spans, counting from 1 as CSS does: the
    /// elements track.
    pub grid_column: Option<(i16, i16)>,
    /// As `grid_column`, for rows: the elements track.
    pub grid_row: Option<(i16, i16)>,
    /// The elements track.
    pub overflow: Overflow,
    /// How opaque the element and its children are, from 0 to 1: the elements track.
    pub opacity: f32,
    /// The color of the element's panel in place of the usual one: the elements track.
    pub background: Option<[u8; 4]>,
    /// The color and width of the element's border: the elements track.
    pub border: Option<([u8; 4], f32)>,
    /// The radius of the element's corners: the elements track.
    pub radius: f32,
    /// How the element's own text looks.
    pub text: TypeStyle,
    pub text_align: TextAlign,
    /// The color of the element's text in place of the design tokens' type color: the text
    /// track.
    pub color: Option<[u8; 4]>,
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
            border: None,
            radius: 0.0,
            text: type_style::BODY,
            text_align: TextAlign::Start,
            color: None,
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
