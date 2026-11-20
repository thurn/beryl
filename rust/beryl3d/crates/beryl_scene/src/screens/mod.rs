//! Screens: what the flat layer shows on each of the web interface's routes, and the sheets
//! that open over them (docs/beryl-3d.md, "The flat layer"). `screen` is one match that calls
//! one function per file, so each screen is built in its own file.

mod bug_report;
mod gallery;
mod game;
pub use game::game_steps;
mod game_over;
mod lobby;
pub(crate) mod opening;
mod settings;
mod start;
mod table;
mod table_entries;
mod table_opening;
mod testing;

pub use bug_report::{BugForm, BugStage, CLICK, DESCRIPTION, span};
pub(crate) use gallery::faces as gallery_faces;
pub use gallery::{GalleryMenu, GalleryQuery, GalleryState};
pub use start::{SEED, StartForm, in_progress, seed};
pub(crate) use start::{checkbox, small};
pub use table_entries::{COULD_NOT_OPEN, EntryMode, INVITE, NAME, TableEntries};

pub use settings::{Concede, GEAR_INSET, corner};
pub use table::sheet_face;

use beryl_protocol::prompt::PromptView;
use beryl_protocol::protocol::{OutcomeView, Side, Status};
use beryl_protocol::script::{Subject, Turn};
use beryl_protocol::{AgentView, PlayerView};

use crate::element::{Element, Viewed};
use crate::faces::FaceList;
use crate::hud::log::LogEntry;
use crate::hud::phone::Occupied;
use crate::inspect::inspector::Inspected;
use crate::inspect::revealed::RevealedFace;
use crate::manual::TableState;
use crate::manual::lobby::Lobby;
use crate::settings::Settings;
use crate::table::Window;
use crate::widgets::Fields;

/// A screen of the client: the web interface's routes (`Main` in `ui/src/App.tsx`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The start screen, where a game is started.
    Start,
    /// A game, from its opening hand to its end.
    Game,
    /// The manual table's lobby, where a table's decks, assists and settings are chosen
    /// before a game.
    Lobby,
    /// The manual table, where both players move cards by hand.
    Table,
    /// The card gallery.
    Gallery,
    /// The testing screen.
    Testing,
}

impl Screen {
    pub const ALL: [Self; 6] = [
        Self::Start,
        Self::Game,
        Self::Lobby,
        Self::Table,
        Self::Gallery,
        Self::Testing,
    ];

    /// The screen's name, as `data/beryl3d/screens.json` writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Game => "game",
            Self::Lobby => "lobby",
            Self::Table => "table",
            Self::Gallery => "gallery",
            Self::Testing => "testing",
        }
    }
}

/// A sheet: what opens over a screen and closes again without leaving it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    Settings,
    BugReport,
}

impl Sheet {
    pub const ALL: [Self; 2] = [Self::Settings, Self::BugReport];

    /// The sheet's name, as `data/beryl3d/screens.json` writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::BugReport => "bug-report",
        }
    }
}

/// Everything a screen function reads.
#[derive(Clone, Copy, Debug)]
pub struct ScreenIn<'a> {
    /// The screen the client is on, which a sheet opens over.
    pub screen: Screen,
    pub window: Window,
    /// The latest view of the game, if one has started.
    pub view: Option<&'a PlayerView>,
    /// The latest view whose script has played out, which the table shows at rest. The web
    /// interface draws the screen around its table from this view (`Playback.update` in
    /// `ui/src/stage/playback.ts`), so a screen that must keep in step with the table, as the
    /// opening does while the hands are dealt and mulliganed, reads it rather than `view`.
    pub played: Option<&'a PlayerView>,
    /// The turn and step the table shows now, as the scripts played so far set them
    /// (`Show::turn_at`), which the step row lights; `None` until the table has shown a view.
    pub turn: Option<Turn>,
    /// The open prompt, if any.
    pub prompt: Option<&'a PromptView>,
    pub settings: &'a Settings,
    /// The sheet open over the screen, if any.
    pub sheet: Option<Sheet>,
    /// The agents a person may play, in the order the host listed them.
    pub agents: &'a [AgentView],
    /// What the person has chosen on the start screen.
    pub start: &'a StartForm,
    /// The start screen's ways onto a manual table: the open form, the saved tables and why
    /// the last table did not open.
    pub entries: &'a TableEntries,
    /// Which samples are open in the gallery.
    pub gallery: GalleryState,
    /// The names of the recordings the testing screen lists, as the host listed them: none
    /// outside a tools build.
    pub recordings: &'a [String],
    /// The seat the testing screen replays recordings from.
    pub seat: Side,
    /// The bug report, while it is open.
    pub bug: &'a BugForm,
    /// What each text field holds, by its name.
    pub fields: &'a Fields,
    /// The card the pointer is over that the inspector shows, if any (`inspect::inspector`).
    pub inspected: Option<&'a Inspected>,
    /// The faces of the match or, on the gallery screen, its stand-in cards.
    pub faces: Option<&'a FaceList>,
    /// The pile whose viewer is open, if any (`inspect::viewer`).
    pub viewing: Option<Viewed>,
    /// The faces of the Revealed Cards pile, the first revealed first, which its viewer lists.
    pub revealed: &'a [RevealedFace],
    /// What the manual table shows over itself: its context menu, command list, ask prompt,
    /// token search and library order tray, and whether its log is open.
    pub table: &'a TableState,
    /// The item of an order prompt's row that the pointer carries along it, which a float draws
    /// in its place (`Client::carry_item`).
    pub carried: Option<Subject>,
    /// The manual table's lobby (`manual::lobby`).
    pub lobby: &'a Lobby,
    /// What has happened in the game so far, oldest first (`hud::log`).
    pub log: &'a [LogEntry],
    /// Whether the folded game log is open.
    pub log_open: bool,
    /// What the settings sheet's concede button does when pressed.
    pub concede: Concede,
    /// Whether the person put the game's result aside to see the battlefield
    /// (`game_over::game_over`).
    pub result_aside: bool,
    /// Whether the opening still shows the coin flip it began with, holding back who starts and
    /// the open decision (`opening::opening`).
    pub spinning: bool,
    /// Where the table's cards rest, on the phone layout, which its prompt and dock keep clear
    /// of (`hud::phone::Occupied`); nothing on the desktop.
    pub occupied: &'a Occupied,
}

/// What the flat layer shows on `screen`.
#[must_use]
pub fn screen(screen: Screen, input: &ScreenIn) -> Element {
    match screen {
        Screen::Start => start::start(input),
        Screen::Game if input.window.phone() => game::game_phone(input),
        Screen::Game => game::game(input),
        Screen::Lobby => lobby::lobby(input),
        Screen::Table => table::table(input),
        Screen::Gallery => gallery::gallery(input),
        Screen::Testing => testing::testing(input),
    }
}

/// What a sheet shows over the screen, given `under`, what the screen itself shows if
/// anything. The settings sheet takes the screen's place; the bug report is drawn over it.
#[must_use]
pub fn sheet(sheet: Sheet, under: Option<Element>, input: &ScreenIn) -> Element {
    match sheet {
        Sheet::Settings => settings::settings(input),
        Sheet::BugReport => bug_report::bug_report(under, input),
    }
}

/// The music that plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Music {
    /// The menu's music, everywhere but in a game and in the gallery.
    Menu,
    /// The game's music, while a game goes on.
    Game,
    /// The sting for a game the viewer won.
    Win,
    /// The sting for a game the viewer did not win.
    Loss,
}

/// The music that plays on `screen` with `view` the latest view of the game, as the web
/// interface's `useMusic` call in `ui/src/App.tsx` chooses it: none in the gallery, the menu's
/// music off the game screen, the game's while it goes on, then a win or a loss once it is
/// over. A game with no view yet is going on.
#[must_use]
pub fn music(screen: Screen, view: Option<&PlayerView>) -> Option<Music> {
    Some(match screen {
        Screen::Gallery => return None,
        Screen::Start | Screen::Lobby | Screen::Table | Screen::Testing => Music::Menu,
        Screen::Game => match view.map(|view| &view.status) {
            Some(Status::Over {
                outcome:
                    OutcomeView::Win {
                        winner: Side::You, ..
                    },
            }) => Music::Win,
            Some(Status::Over { .. }) => Music::Loss,
            _ => Music::Game,
        },
    })
}

/// The dimmed table, `.overlay-backdrop`: the whole window in the backdrop's color, which the
/// pointer goes through. The opening and game over draw it under what they show.
fn backdrop(input: &ScreenIn) -> Element {
    let mut backdrop = crate::element::row()
        .name("scrim")
        .width(input.window.size.width)
        .height(input.window.size.height)
        .background(crate::tokens::color::BACKDROP)
        .passes(true);
    backdrop.anchor = Some(crate::element::Anchor::Point(
        glam::Vec2::ZERO,
        glam::Vec2::ZERO,
    ));
    backdrop
}

/// `screen` laid over the whole window, as the web's desktop screens fill the page: the flat
/// layer's root puts a screen in its bottom right corner inside the game screen's dock offsets,
/// which would centre what the screen holds left of and above the window's middle.
fn whole_window(input: &ScreenIn, screen: crate::element::Element) -> crate::element::Element {
    let mut screen = screen
        .width(input.window.size.width)
        .height(input.window.size.height);
    screen.anchor = Some(crate::element::Anchor::Point(
        glam::Vec2::ZERO,
        glam::Vec2::ZERO,
    ));
    screen
}

/// Whether the window takes the phone layout, by the selector the interface checks use
/// (`Layout::for_size`). Each screen's public function asks this and calls its portrait
/// alternate.
fn phone(input: &ScreenIn) -> bool {
    crate::agent::Layout::for_size(input.window.size) == crate::agent::Layout::Phone
}

/// A portrait screen named `name`, which shows `content` in one column over the safe area: the
/// screen is clear and anchored over the whole window, and `content` stands inside its safe
/// area, the gutter in from its sides and foot, at most `phone::COLUMN` wide in the window's
/// middle. `content` scrolls when it is taller than that, as it is while the system keyboard
/// shortens the window, so that every action stays in reach; it is cut at the safe area, so
/// nothing scrolls under the status bar or the home indicator.
fn phone_screen(
    name: &str,
    input: &ScreenIn,
    content: crate::element::Element,
) -> crate::element::Element {
    use crate::data::space::flat::phone::{COLUMN, GUTTER};
    let window = input.window;
    let safe = window.safe_area;
    let across = window.size.width - safe.left - safe.right - 2.0 * GUTTER;
    let extra = ((across - COLUMN) / 2.0).max(0.0);
    let mut content = content
        .grow(1.0)
        .shrink(1.0)
        .min_height(0.0)
        .align_self(crate::element::Align::Stretch)
        .overflow(crate::element::Overflow::Scroll);
    content.style.padding = [0.0, GUTTER + extra, GUTTER, GUTTER + extra];
    let mut screen = crate::element::column()
        .name(name)
        .background([0; 4])
        .border(([0; 4], 0.0))
        .width(window.size.width)
        .height(window.size.height)
        .child(content);
    screen.style.padding = [safe.top, safe.right, safe.bottom, safe.left];
    // The flat layer's root puts a screen in its bottom right corner inside the dock's offsets,
    // so the screen is anchored at the window's top left instead.
    screen.anchor = Some(crate::element::Anchor::Point(
        glam::Vec2::ZERO,
        glam::Vec2::ZERO,
    ));
    screen
}

/// `button` as a phone's action: as wide as its column, `phone::ACTION_HEIGHT` tall, with
/// larger type, as the web's phone dock draws its buttons (`.phone-opening .dock .button`).
fn phone_action(mut button: crate::element::Element) -> crate::element::Element {
    use crate::data::space::flat::phone::{ACTION_HEIGHT, ACTION_TEXT};
    button.style.height = Some(ACTION_HEIGHT);
    button.style.text.size = ACTION_TEXT;
    button.style.min_width = None;
    button.style.align_self = Some(crate::element::Align::Stretch);
    button.style.shrink = 0.0;
    button
}
