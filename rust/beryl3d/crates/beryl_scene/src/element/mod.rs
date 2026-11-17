//! Elements of the flat layer (docs/beryl-3d.md, "Elements"): nodes with a role, a name, a
//! style, text, what a press on them means, and children. Screens are plain functions that
//! return elements; the client keeps the tree they return and lays it out (`layout`).

mod keys;
mod style;

use std::ops::Range;

use beryl_protocol::prompt::{ButtonId, Click};
use beryl_protocol::protocol::{Side, Step, ZoneName};
use beryl_protocol::script::Subject;
use glam::Vec2;

pub use keys::{Chord, Shortcuts, describe};
pub use style::{Align, Direction, Display, Justify, Overflow, Position, Style, TextAlign, Track};

use crate::frame::{ElementId, Role};
use crate::input::{FaceId, FaceSize};
use crate::screens::{Screen, Sheet};
use crate::settings::Setting;
use crate::text::TypeStyle;
use crate::widgets::{FieldState, IconId, MenuId};

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
    /// Start a game with what the start screen holds: the chosen opponent, the seed and the
    /// fair-deal box. With a game in progress the first press asks to confirm, and the next
    /// concedes that game before starting the new one.
    Start,
    /// Choose the `n`th agent the host listed (`Input::Agents`), counting from 0, as the
    /// opponent of the next game.
    Agent(u16),
    /// Turn the start screen's fair-deal box on or off.
    FairDeal,
    /// Save every bug report to one file (`Request::ExportBugs`).
    ExportBugs,
    /// Replay the `recording`th recording the host listed (`Input::Recordings`), counting
    /// from 0, from the seat the testing screen's toggle shows.
    Replay { recording: u16 },
    /// Set the seat the testing screen replays recordings from.
    Seat(Side),
    /// A row of an open menu, counting from 0 over all its groups.
    Menu(MenuId, u16),
    /// Open one of the gallery's example context menus.
    GalleryMenu(crate::screens::GalleryMenu),
    /// Show one of the gallery's example command searches, or close its preview.
    GalleryQuery(Option<crate::screens::GalleryQuery>),
    /// Give the keys to an element, such as a text field.
    Focus(ElementId),
    /// The text field `ElementId` now holds this text: an edit made in it, which
    /// `Client::press` keeps in the field's `FieldState`.
    Field(ElementId, String),
    /// Close something shown over the screen.
    Dismiss(Dismiss),
    /// Open the viewer of a pile's cards, as a press on a pile does.
    View(Viewed),
    /// Give the manual table's command at this index of `manual::commands::COMMANDS` to what
    /// the pointer is on, as a key of the table does. The flag is whether Shift made the key's
    /// token face down.
    Command(u16, bool),
    /// Something the manual table shows over itself: its command list, ask prompt, token
    /// search, library order tray or log.
    Table(crate::manual::TablePress),
    /// One of the bug report's controls.
    Bug(BugPress),
    /// One of the manual table's lobby controls.
    Lobby(LobbyPress),
    /// Open the folded game log, or close it (`hud::log`).
    Log,
    /// Take back the viewer's latest answer that can be taken back (`Client::undo`).
    Undo,
}

/// What a `Press::Lobby` does (`screens::lobby`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyPress {
    /// Open a solo table's lobby, from the start screen.
    Solo,
    /// Open the deck picker on the saved decks ('Change').
    Change,
    /// Open the deck picker on the field a decklist is pasted into ('Paste decklist').
    Paste,
    /// Load the saved deck at this index of the deck picker's list.
    Pick(u16),
    /// Read the pasted decklist, save it under the name typed and load it ('Import').
    Import,
    /// Close the deck picker.
    ClosePicker,
    /// Open the sideboard swap, or close it.
    Sideboard,
    /// Move one copy of the card at index `at` of the loaded deck's main deck to its sideboard,
    /// or with `from_side`, of its sideboard to its main deck.
    Swap { from_side: bool, at: u16 },
    /// Unload the loaded deck.
    Unload,
    /// Mark the guest ready, or no longer ready.
    Ready,
    /// Turn the assist at this index of `manual::lobby::ASSISTS` on or off.
    Assist(u8),
    /// Ask for the starting life and hand size the host's fields hold.
    SaveSettings,
    /// Start the table's game.
    Start,
    /// Leave the table and go back to the start screen.
    Leave,
}

/// What a `Press::Bug` does (`screens::bug_report`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BugPress {
    /// Report the whole window instead of a region dragged out over it.
    Whole,
    /// Turn the "AI play" box on or off.
    Ai,
    /// Save the report with the description typed.
    Save,
}

/// What the viewer shows the cards of (`inspect::viewer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewed {
    /// `side`'s library, graveyard or exile.
    Pile(Side, ZoneName),
    /// The faces of the Revealed Cards pile.
    Revealed,
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
    /// Centred above a scene object's box as the table draws it, with the element's bottom
    /// edge the given number of logical points above the box's highest point, as the web's
    /// `translate(-50%, calc(-100% - 5px))` stands a label over a card.
    Above(Subject, f32),
    /// Centred below a scene object's box as the table draws it, with the element's top edge
    /// the given number of logical points below the box's lowest point.
    Below(Subject, f32),
    /// A point of the window where a menu opens, in logical points, as `Menu.tsx` places one:
    /// its top left corner stands at the point, or its right or bottom edge does where the menu
    /// would otherwise pass the window's edge, and it is kept `flat.menu_edge` points inside the
    /// window (`layout::menu`).
    Menu(Vec2),
    /// Beside the menu its parent row is in, as a submenu stands: on the menu's right if it
    /// fits there and on its left if not, level with its row, and inside the window. It is shown
    /// only while its row is open, under the pointer or opened by the right arrow key.
    Submenu,
    /// Beside the element a tooltip explains, as `tips.css` places a tip: centred above it,
    /// or below it where there is no room above. It is shown only once the pointer has rested
    /// on that element for `flat.tooltip_delay` milliseconds (`layout::tip`).
    Tip,
}

/// What an element shows in its box besides its text and children.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shown {
    /// A card face, drawn flat.
    Face(FaceId, FaceSize),
    /// A tapped card face, drawn flat with its long side horizontal.
    TappedFace(FaceId, FaceSize),
    /// The scene object itself, placed in the element's box, so that it looks and moves as it
    /// does on the table.
    Object(Subject),
    /// An icon `size` logical points on a side, centred in the element's content box and
    /// tinted with its text color.
    Icon(IconId, u16),
    /// `side`'s life orb (`art::orb`), lit as the active player's if `active`, its art filling
    /// the element's content box at `art::orb::SCALES[scale]` points to a unit.
    Orb { side: Side, active: bool, scale: u8 },
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag for each state a builder sets, as `StateBits` holds them resolved"
)]
pub struct Element {
    pub role: Role,
    /// What locators and the tree printout call it. A text or a button is named by its words
    /// unless it is given a name.
    pub name: String,
    pub style: Style,
    /// The words it shows, laid out inside its padding.
    pub text: Option<String>,
    /// Runs of `text` styled apart from the rest, laid out in one paragraph with it.
    pub spans: Vec<Span>,
    pub press: Option<Press>,
    /// Whether a press does anything now.
    pub enabled: bool,
    /// Whether it takes keys: a press on it gives it the focus (`layout::focus`).
    pub focus: bool,
    /// Whether it is on or chosen, as a toggle that is on is. It is drawn so, and its resolved
    /// element has the `SELECTED` state.
    pub selected: bool,
    /// Whether it is the button a prompt or screen expects to be pressed (`widgets::primary`).
    /// Its resolved element has the `PRIMARY` state, so that the tree names it.
    pub primary: bool,
    /// Where it stands if it is placed at a point of the window or on a scene object rather
    /// than in its parent's layout. It is laid out at its content's size (`layout::anchor`).
    pub anchor: Option<Anchor>,
    /// A face or a scene object shown in its box: the furniture and cards track.
    pub shows: Option<Shown>,
    /// The keyboard shortcuts the screen answers, given on its top element.
    pub shortcuts: Shortcuts,
    /// For a text field, what it holds and where its caret is (`widgets::field`), from which
    /// the layer starts editing it when it takes the focus.
    pub field: Option<Box<FieldState>>,
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
            selected: false,
            primary: false,
            anchor: None,
            shows: None,
            shortcuts: Shortcuts::default(),
            field: None,
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
            && self.selected == other.selected
            && self.primary == other.primary
            && self.anchor == other.anchor
            && self.shows == other.shows
            && self.shortcuts == other.shortcuts
            && self.field == other.field
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
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    #[must_use]
    pub fn on_press(mut self, press: Press) -> Self {
        self.press = Some(press);
        self
    }

    /// Answers the shortcut `binding`, written as the web interface writes one (`keys`), with
    /// `press`. Only the screen's top element's shortcuts are answered.
    #[must_use]
    pub fn shortcut(mut self, binding: &str, press: Press) -> Self {
        self.shortcuts.0.push((binding.to_owned(), press));
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

/// A region that only holds its children and draws nothing of its own, not even the panel a
/// screen's top element is drawn on. When it enters the screen, each of its children fades in
/// on its own (`layout::reconcile`), so that a child anchored at a point of the window stands
/// there at once.
#[must_use]
pub fn holder() -> Element {
    row().background([0; 4]).border(([0; 4], 0.0))
}

impl Element {
    /// Whether it draws nothing of its own: no words, no icon or face, and a clear panel, as a
    /// `holder` is.
    #[must_use]
    pub fn draws_nothing(&self) -> bool {
        self.text.is_none()
            && self.shows.is_none()
            && self.style.background.is_some_and(|tint| tint[3] == 0)
            && self
                .style
                .border
                .is_none_or(|(edge, width)| edge[3] == 0 || width == 0.0)
    }
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
    /// The inspector: the card under the pointer at full size beside the table
    /// (`inspect::inspector`).
    pub const INSPECTOR: FloatId = FloatId(28);
    /// The gear at the window's top right that opens the settings sheet.
    pub const GEAR: FloatId = FloatId(29);
    /// The "Revealed Cards" pill over the Revealed Cards pile, with the cross that dismisses
    /// the pile (`inspect::revealed`).
    pub const REVEALED: FloatId = FloatId(30);
    /// The waiting trigger the pointer carries along an order prompt's row (`Client::carry_item`).
    pub const CARRIED_ITEM: FloatId = FloatId(31);
    /// The step banner between the opponent's orb and their permanents, such as "Combat" over
    /// "Blockers declared" (`hud::banner`).
    pub const STEP_BANNER: FloatId = FloatId(32);
    /// The count of cards scrolled off the left end and the right end of the viewer's hand.
    pub const HAND_COUNT: [FloatId; 2] = [FloatId(34), FloatId(35)];
    /// The count of items scrolled off the stack's bottom, at its left, and its top, at its
    /// right.
    pub const STACK_COUNT: [FloatId; 2] = [FloatId(36), FloatId(37)];
    /// The word under the opponent's permanent that refused a click by holding the viewer to
    /// sorcery speed (`Client::refusal_frame`).
    pub const REFUSAL: FloatId = FloatId(38);
}
