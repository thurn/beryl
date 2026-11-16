//! Screens: what the flat layer shows on each of the web interface's routes, and the sheets
//! that open over them (docs/beryl-3d.md, "The flat layer"). `screen` is one match that calls
//! one function per file, so each screen is built in its own file.

mod bug_report;
mod gallery;
mod game;
mod game_over;
mod opening;
mod settings;
mod start;
mod table;
mod testing;

pub use bug_report::{BugForm, BugStage, CLICK, DESCRIPTION, span};
pub(crate) use gallery::faces as gallery_faces;
pub use gallery::{GalleryMenu, GalleryQuery, GalleryState};
pub use start::{SEED, StartForm, in_progress, seed};

pub use settings::{GEAR_INSET, gear};

use beryl_protocol::prompt::PromptView;
use beryl_protocol::protocol::{OutcomeView, Side, Status};
use beryl_protocol::script::Subject;
use beryl_protocol::{AgentView, PlayerView};

use crate::element::{Element, Viewed};
use crate::faces::FaceList;
use crate::frame::ElementId;
use crate::inspect::inspector::Inspected;
use crate::inspect::revealed::RevealedFace;
use crate::manual::menus::OpenMenu;
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
    /// The manual table, where both players move cards by hand.
    Table,
    /// The card gallery.
    Gallery,
    /// The testing screen.
    Testing,
}

impl Screen {
    pub const ALL: [Self; 5] = [
        Self::Start,
        Self::Game,
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
    /// The open prompt, if any.
    pub prompt: Option<&'a PromptView>,
    pub settings: &'a Settings,
    /// The element the pointer is over, if any.
    pub hovered: Option<ElementId>,
    /// The sheet open over the screen, if any.
    pub sheet: Option<Sheet>,
    /// The agents a person may play, in the order the host listed them.
    pub agents: &'a [AgentView],
    /// What the person has chosen on the start screen.
    pub start: &'a StartForm,
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
    /// The manual table's open context menu, if any (`manual::menus`).
    pub table_menu: Option<&'a OpenMenu>,
    /// The item of an order prompt's row that the pointer carries along it, which a float draws
    /// in its place (`Client::carry_item`).
    pub carried: Option<Subject>,
}

/// What the flat layer shows on `screen`.
#[must_use]
pub fn screen(screen: Screen, input: &ScreenIn) -> Element {
    match screen {
        Screen::Start => start::start(input),
        Screen::Game => game::game(input),
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
        Screen::Start | Screen::Table | Screen::Testing => Music::Menu,
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
