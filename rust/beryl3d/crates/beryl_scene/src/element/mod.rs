//! Elements of the flat layer (docs/beryl-3d.md, "Elements"): nodes with a role, a name, a
//! style, text, what a press on them means, and children. Screens are plain functions that
//! return elements; the client keeps the tree they return and lays it out (`layout`).

mod style;

use std::ops::Range;

use beryl_protocol::prompt::{ButtonId, Click};
use beryl_protocol::protocol::Step;
use beryl_protocol::script::Subject;
use glam::Vec2;

pub use style::{Align, Direction, Display, Justify, Overflow, Position, Style, TextAlign, Track};

use crate::frame::{ElementId, Role};
use crate::input::{FaceId, FaceSize};
use crate::screens::{Screen, Sheet};
use crate::settings::Setting;
use crate::text::TypeStyle;
use crate::widgets::MenuId;

/// What a press on an element means. A value, not a closure, so that an unchanged element
/// compares equal and a test can print it. `Client::press` turns it into a `SessionCommand`, a
/// `Request` or a change of screen, sheet or settings, adding what a screen function does not
/// know, such as the open decision's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Press {
    /// One of the open prompt's buttons.
    Button(ButtonId),
    /// Any click on the open prompt other than a button: a card, an ability or a player, a
    /// number, a catalog entry, or a card dropped into a pile.
    Prompt(Click),
    /// Go to a screen.
    Open(Screen),
    /// Open a sheet over the screen, or close the open one.
    Sheet(Option<Sheet>),
    /// Change a setting.
    Setting(Setting),
    /// Turn the stop at `step` on or off, on the player's own turns or, if `theirs`, on the
    /// opponent's.
    Stop { step: Step, theirs: bool },
    /// Concede the game.
    Concede,
    /// Start a game against the `agent`th agent the host listed (`Input::Agents`), counting
    /// from 0.
    Start { agent: u16 },
    /// A row of an open menu, counting from 0 over all its groups.
    Menu(MenuId, u16),
    /// Give the keys to an element, such as a text field.
    Focus(ElementId),
    /// Close something shown over the screen.
    Dismiss(Dismiss),
}

/// What a `Press::Dismiss` closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dismiss {
    /// The cards an opponent's effect revealed.
    Revealed,
    /// The open menu.
    Menu,
    /// The browser of a pile's cards.
    Viewer,
}

/// Where an element stands when it is placed at a point of the window rather than in its
/// parent's layout. The second vector says which point of the element's box lies there: (0, 0)
/// is its top left corner and (1, 1) its bottom right corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// A point of the window, in logical points.
    Point(Vec2, Vec2),
    /// The centre of a scene object as the camera projects it.
    Object(Subject, Vec2),
}

/// What an element shows in its box besides its text and children.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shown {
    /// A card face, drawn flat.
    Face(FaceId, FaceSize),
    /// The scene object itself, placed in the element's box, so that it looks and moves as it
    /// does on the table.
    Object(Subject),
}

/// A styled run of an element's text.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// The run's bytes in the element's `text`.
    pub range: Range<u32>,
    pub style: TypeStyle,
    /// The run's color in place of the element's own.
    pub color: Option<[u8; 4]>,
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
    /// Runs of `text` styled apart from the rest: the text track.
    pub spans: Vec<Span>,
    pub press: Option<Press>,
    /// Whether a press does anything now.
    pub enabled: bool,
    /// Whether it takes keys: the widgets track.
    pub focus: bool,
    /// Where it stands if it is placed at a point of the window or on a scene object rather
    /// than in its parent's layout. It is laid out at its content's size (`layout::anchor`).
    pub anchor: Option<Anchor>,
    /// A face or a scene object shown in its box: the furniture and cards track.
    pub shows: Option<Shown>,
    pub children: Vec<Element>,
}

impl Element {
    /// An element of `role` with no name, the default style and nothing in it.
    #[must_use]
    pub fn new(role: Role) -> Self {
        Self {
            role,
            name: String::new(),
            style: Style::default(),
            text: None,
            spans: Vec::new(),
            press: None,
            enabled: true,
            focus: false,
            anchor: None,
            shows: None,
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
            && self.spans == other.spans
            && self.press == other.press
            && self.enabled == other.enabled
            && self.focus == other.focus
            && self.anchor == other.anchor
            && self.shows == other.shows
    }

    #[must_use]
    pub fn name(mut self, name: &str) -> Self {
        name.clone_into(&mut self.name);
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

/// Which float of the flat layer an element is: a small element placed at a point of the
/// window rather than in a screen's layout (`Layer::float`). The ids every part of the client
/// uses are in `floats`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FloatId(pub u16);

/// The ids of the flat layer's floats. Where a float comes in pairs or more, the arrays are
/// indexed by side first, the viewer's at 0 and the opponent's at 1.
pub mod floats {
    use super::FloatId;

    /// The banner in the middle of the window that says whose turn begins.
    pub const BANNER: FloatId = FloatId(0);
    /// The pill above a player that says they are looking at cards.
    pub const SCRY_PILL: [FloatId; 2] = [FloatId(1), FloatId(2)];
    /// The coin shown when the game decides who plays first.
    pub const COIN: FloatId = FloatId(3);
    /// A player's life total.
    pub const LIFE: [FloatId; 2] = [FloatId(4), FloatId(5)];
    /// The number that floats off a life total when it changes.
    pub const LIFE_CHANGE: [FloatId; 2] = [FloatId(6), FloatId(7)];
    /// A player's mana pool.
    pub const POOL: [FloatId; 2] = [FloatId(8), FloatId(9)];
    /// The count on a pile, by side, then library, graveyard and exile.
    pub const PILE_COUNT: [[FloatId; 3]; 2] = [
        [FloatId(10), FloatId(11), FloatId(12)],
        [FloatId(13), FloatId(14), FloatId(15)],
    ];
    /// The count of cards scrolled off one end of a battlefield row: by side, then by row in
    /// `RowName`'s order (creatures, others, lands), then the left end and the right end.
    pub const ROW_COUNT: [[[FloatId; 2]; 3]; 2] = [
        [
            [FloatId(16), FloatId(17)],
            [FloatId(18), FloatId(19)],
            [FloatId(20), FloatId(21)],
        ],
        [
            [FloatId(22), FloatId(23)],
            [FloatId(24), FloatId(25)],
            [FloatId(26), FloatId(27)],
        ],
    ];
}
