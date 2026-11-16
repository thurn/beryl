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
    /// The color and width of the element's border, which takes room inside its box as CSS's
    /// does.
    pub border: Option<([u8; 4], f32)>,
    /// The radius of the element's panel's corners, or 0 for the usual radius: a screen's
    /// panel's or a button's, and square on any other element.
    pub radius: f32,
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
            fit: false,
            passes: false,
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
