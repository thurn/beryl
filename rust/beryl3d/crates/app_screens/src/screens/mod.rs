//! Screens: what the flat layer shows on each of the web interface's routes, and the sheets
//! that open over them (docs/beryl-3d.md, "The flat layer"). `screen` is one match that calls
//! one function per file, so each screen is built in its own file.

pub mod bug_report;
mod gallery;
pub mod settings;
pub mod start;
pub mod testing;

pub use gallery::faces as gallery_faces;
pub use gallery::{GalleryMenu, GalleryQuery, GalleryState};
pub use game_manual::screens::table_entries::{
    COULD_NOT_OPEN, EntryMode, INVITE, NAME, TableEntries,
};

pub use game_hud::hud::steps::steps as game_steps;
pub use game_hud::navigation::{Screen, Sheet};
use game_hud::screens::game;
pub use game_play::sound::Music;
pub use settings::{GEAR_INSET, corner};

use beryl_protocol::prompt::PromptView;
use beryl_protocol::protocol::{OutcomeView, Side, Status};
use beryl_protocol::script::{Subject, Turn};
use beryl_protocol::{PlayerView, StopsSetting};
use game_hud::inspect::Viewed;

use game_hud::hud::HudIn;
use game_hud::hud::placement_scene::PlacementScene;
use game_hud::inspect::inspector::Inspected;
use game_hud::inspect::revealed::RevealedFace;
use game_table::faces::FaceList;
use game_table::table::Window;
use kit_ui::element::{Action, Element};
use kit_ui::widgets::Fields;

/// Everything a screen function reads.
#[derive(Clone, Copy, Debug)]
pub struct ScreenIn<'a> {
    /// The screen the client is on, which a sheet opens over.
    pub screen: Screen,
    pub window: Window,
    pub profile: game_table::profiles::Profile<'a>,
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
    /// Whether full control is enabled.
    pub full_control: bool,
    /// Where the player stops with the stack empty.
    pub stops: StopsSetting,
    /// The sheet open over the screen, if any.
    pub sheet: Option<Sheet>,
    /// Which samples are open in the gallery.
    pub gallery: GalleryState,
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
    /// The item of an order prompt's row that the pointer carries along it, which a float draws
    /// in its place (`Client::carry_item`).
    pub carried: Option<Subject>,
    /// Whether the opening still shows the coin flip it began with, holding back who starts and
    /// the open decision (`opening::opening`).
    pub spinning: bool,
    /// The scene used to place controls in the phone layout.
    pub placement_scene: Option<&'a PlacementScene>,
}

impl<'a> ScreenIn<'a> {
    /// What the game furniture and inspector read from this screen.
    #[must_use]
    pub fn hud(&self) -> HudIn<'a> {
        HudIn {
            window: self.window,
            profile: self.profile,
            view: self.view,
            played: self.played,
            turn: self.turn,
            prompt: self.prompt,
            stops: StopsSetting {
                full_control: self.full_control,
                ..self.stops
            },
            sheet: self.sheet,
            inspected: self.inspected,
            faces: self.faces,
            viewing: self.viewing,
            revealed: self.revealed,
            carried: self.carried,
            spinning: self.spinning,
            placement_scene: self.placement_scene,
        }
    }
}

/// Composes the game and gallery routes.
#[must_use]
pub fn screen(
    input: &ScreenIn,
    furniture: game::Furniture,
    log: Option<Element>,
    result: Option<Element>,
) -> Option<Element> {
    Some(match input.screen {
        Screen::Start | Screen::Testing | Screen::Lobby | Screen::Table => return None,
        Screen::Game => game::game(
            &input.hud(),
            furniture,
            log,
            result,
            Action::new(settings::Message::Open),
        ),
        Screen::Gallery => gallery::gallery(input),
    })
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

mod message;
pub use message::ScreenPress;
