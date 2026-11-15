//! Elements of the flat layer (docs/beryl-3d.md, "Elements"): nodes with a role, a name, a
//! style, text, what a press on them means, and children. Screens are plain functions that
//! return elements; the client keeps the tree they return and lays it out (`layout`).

mod prompt;

use beryl_protocol::prompt::ButtonId;

pub use prompt::prompt_bar;

use crate::frame::Role;
use crate::text::TypeStyle;
use crate::tokens::{size, type_style};

/// What a press on an element means. A value, not a closure, so that an unchanged element
/// compares equal and a test can print it. The client turns it into a `SessionCommand`,
/// adding what a screen function does not know, such as the open decision's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// One of the open prompt's buttons.
    Button(ButtonId),
}

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

/// An element's layout and type, with CSS's names. Sizes are in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub direction: Direction,
    pub gap: f32,
    /// Top, right, bottom and left.
    pub padding: [f32; 4],
    pub grow: f32,
    pub align_items: Option<Align>,
    pub justify_content: Option<Justify>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// How the element's own text looks.
    pub text: TypeStyle,
    pub text_align: TextAlign,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            direction: Direction::Row,
            gap: 0.0,
            padding: [0.0; 4],
            grow: 0.0,
            align_items: None,
            justify_content: None,
            width: None,
            height: None,
            text: type_style::BODY,
            text_align: TextAlign::Start,
        }
    }
}

/// A node of the flat layer's tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub role: Role,
    /// What locators and the tree printout call it. A text or a button is named by its words
    /// unless it is given a name.
    pub name: String,
    pub style: Style,
    /// The words it shows, laid out inside its padding.
    pub text: Option<String>,
    pub press: Option<Press>,
    /// Whether a press does anything now.
    pub enabled: bool,
    pub children: Vec<Element>,
}

impl Element {
    fn new(role: Role) -> Self {
        Self {
            role,
            name: String::new(),
            style: Style::default(),
            text: None,
            press: None,
            enabled: true,
            children: Vec::new(),
        }
    }

    /// Whether `self` and `other` are the same apart from their children.
    #[must_use]
    pub fn same_node(&self, other: &Self) -> bool {
        self.role == other.role
            && self.name == other.name
            && self.style == other.style
            && self.text == other.text
            && self.press == other.press
            && self.enabled == other.enabled
    }

    #[must_use]
    pub fn name(mut self, name: &str) -> Self {
        name.clone_into(&mut self.name);
        self
    }

    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.style.gap = gap;
        self
    }

    /// The same padding on every side.
    #[must_use]
    pub fn padding(mut self, padding: f32) -> Self {
        self.style.padding = [padding; 4];
        self
    }

    #[must_use]
    pub fn grow(mut self, grow: f32) -> Self {
        self.style.grow = grow;
        self
    }

    #[must_use]
    pub fn align_items(mut self, align: Align) -> Self {
        self.style.align_items = Some(align);
        self
    }

    #[must_use]
    pub fn justify_content(mut self, justify: Justify) -> Self {
        self.style.justify_content = Some(justify);
        self
    }

    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.style.width = Some(width);
        self
    }

    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.style.height = Some(height);
        self
    }

    /// How the element's text looks.
    #[must_use]
    pub fn style(mut self, style: TypeStyle) -> Self {
        self.style.text = style;
        self
    }

    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    #[must_use]
    pub fn on_press(mut self, press: Press) -> Self {
        self.press = Some(press);
        self
    }

    #[must_use]
    pub fn child(mut self, child: Element) -> Self {
        self.children.push(child);
        self
    }

    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Element>) -> Self {
        self.children.extend(children);
        self
    }
}

/// A region that lays out its children left to right.
#[must_use]
pub fn row() -> Element {
    Element::new(Role::Region)
}

/// A region that lays out its children top to bottom.
#[must_use]
pub fn column() -> Element {
    let mut element = Element::new(Role::Region);
    element.style.direction = Direction::Column;
    element
}

/// Words, named by themselves.
#[must_use]
pub fn text(words: &str) -> Element {
    let mut element = Element::new(Role::Text);
    words.clone_into(&mut element.name);
    element.text = Some(words.to_owned());
    element
}

/// A button showing `label`, named by it: as tall as every action button, with its label
/// centred.
#[must_use]
pub fn button(label: &str) -> Element {
    let mut element = Element::new(Role::Button);
    label.clone_into(&mut element.name);
    element.text = Some(label.to_owned());
    element.style.text = type_style::BUTTON;
    element.style.text_align = TextAlign::Center;
    element.style.height = Some(size::BUTTON_HEIGHT);
    element.style.padding = [0.0, size::BUTTON_PADDING, 0.0, size::BUTTON_PADDING];
    element
}
