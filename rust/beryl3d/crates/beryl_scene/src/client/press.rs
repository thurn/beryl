//! Dispatches messages received from the flat layer and the client's own controls.

use beryl_protocol::prompt::{ButtonId, Click};
use beryl_protocol::protocol::{Side, Step};

use super::Client;
use crate::command::{Request, SessionCommand};
use crate::element::Action;
use crate::faces::Faces;
use crate::frame::ElementId;
use crate::hud::GamePress;
use crate::inspect::Viewed;
use crate::layout::DismissMenu;
use crate::manual::{LobbyPress, TableCommand, TablePress};
use crate::screens::{self, BugPress, Concede, GalleryState, Screen, ScreenPress, Sheet};
use crate::settings::Setting;
use crate::widgets::{MenuChoice, MenuId};

use super::stops::toggled;

/// What a press on an element means. The client turns it into a `SessionCommand`, a
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
    /// Show the ended game's result over the table, or put it aside to see the battlefield
    /// (`screens::game_over`).
    Result(bool),
    /// Take back the viewer's latest answer that can be taken back (`Client::undo`).
    Undo,
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

impl Client {
    /// Carries a flat-layer message to the feature that handles it.
    pub fn press(&mut self, action: &Action) {
        if let Some(press) = action.downcast_ref::<Press>() {
            self.dispatch_press(press);
        } else if let Some(&press) = action.downcast_ref::<ScreenPress>() {
            let press = match press {
                ScreenPress::Open(screen) => Press::Open(screen),
                ScreenPress::Sheet(sheet) => Press::Sheet(sheet),
                ScreenPress::Concede => Press::Concede,
                ScreenPress::Start => Press::Start,
                ScreenPress::Agent(agent) => Press::Agent(agent),
                ScreenPress::FairDeal => Press::FairDeal,
                ScreenPress::ExportBugs => Press::ExportBugs,
                ScreenPress::Replay { recording } => Press::Replay { recording },
                ScreenPress::Seat(side) => Press::Seat(side),
                ScreenPress::GalleryMenu(kind) => Press::GalleryMenu(kind),
                ScreenPress::GalleryQuery(query) => Press::GalleryQuery(query),
                ScreenPress::DismissMenu => Press::Dismiss(Dismiss::Menu),
                ScreenPress::Bug(kind) => Press::Bug(kind),
                ScreenPress::Log => Press::Log,
                ScreenPress::Undo => Press::Undo,
            };
            self.dispatch_press(&press);
        } else if let Some(&setting) = action.downcast_ref::<Setting>() {
            self.set(setting);
        } else if let Some(&click) = action.downcast_ref::<Click>() {
            self.click(click);
        } else if let Some(&press) = action.downcast_ref::<GamePress>() {
            let press = match press {
                GamePress::OpenStart => Press::Open(Screen::Start),
                GamePress::OpenSettings => Press::Sheet(Some(Sheet::Settings)),
                GamePress::Stop { step, theirs } => Press::Stop { step, theirs },
                GamePress::Log => Press::Log,
                GamePress::Result(shown) => Press::Result(shown),
                GamePress::View(viewed) => Press::View(viewed),
                GamePress::DismissRevealed => Press::Dismiss(Dismiss::Revealed),
                GamePress::DismissViewer => Press::Dismiss(Dismiss::Viewer),
            };
            self.dispatch_press(&press);
        } else if let Some(&press) = action.downcast_ref::<TablePress>() {
            self.table_press(press);
        } else if let Some(&press) = action.downcast_ref::<LobbyPress>() {
            self.press_lobby(press);
        } else if let Some(&TableCommand(index, shift)) = action.downcast_ref::<TableCommand>() {
            self.table_key(index, shift);
        } else if let Some(&MenuChoice(menu, row)) = action.downcast_ref::<MenuChoice>() {
            self.dispatch_press(&Press::Menu(menu, row));
        } else if action.downcast_ref::<DismissMenu>().is_some() {
            self.dispatch_press(&Press::Dismiss(Dismiss::Menu));
        }
    }

    /// Carries out what a press on an element means: a command for the session, a request of
    /// the host, or a change of screen, sheet or settings. Every `Press` is interpreted here
    /// and nowhere else.
    pub(crate) fn dispatch_press(&mut self, press: &Press) {
        match *press {
            Press::Button(button) => self.click(Click::Button { button }),
            Press::Prompt(click) => self.click(click),
            Press::Open(screen) => {
                self.first_view_opens = None;
                self.screen = screen;
                self.gallery = GalleryState::default();
                if screen == Screen::Gallery {
                    self.gallery_faces
                        .get_or_insert_with(|| Faces::new(screens::gallery_faces()));
                }
                self.set_sheet(None);
            }
            Press::Sheet(sheet) => self.set_sheet(sheet),
            Press::Setting(setting) => self.set(setting),
            Press::Stop { step, theirs } => {
                let stops = &mut self.settings.stops;
                let set = if theirs {
                    &mut stops.theirs
                } else {
                    &mut stops.yours
                };
                *set = toggled(*set, step);
                self.save_stops();
            }
            // The first press only asks to confirm, as the web's `ConcedeButton` does.
            Press::Concede if self.concede == Concede::Asks => {
                self.concede = Concede::Confirms;
                self.show_screen();
            }
            Press::Concede => {
                self.set_sheet(None);
                if let Some(session) = self.session {
                    self.commands.push_back(SessionCommand::Concede(session));
                }
            }
            Press::Start => self.start_game(),
            Press::Agent(agent) => {
                self.start.agent = Some(agent);
                self.show_screen();
            }
            Press::FairDeal => {
                self.start.fair = !self.start.fair;
                self.show_screen();
            }
            Press::ExportBugs => self.requests.push_back(Request::ExportBugs),
            Press::Replay { recording } => self.replay(recording),
            Press::Seat(seat) => self.set_seat(seat),
            Press::Focus(id) => self.layer.focus(Some(id)),
            Press::Field(id, ref text) => self.keep_field(id, text),
            Press::View(viewed) => self.view(Some(viewed)),
            Press::Dismiss(Dismiss::Viewer) => self.dismiss_viewer(),
            Press::Bug(press) => self.bug_press(press),
            Press::Dismiss(Dismiss::Revealed) => self.dismiss_revealed(),
            Press::GalleryMenu(kind) => {
                let at = self.owned.mouse.unwrap_or_else(|| {
                    glam::Vec2::new(self.window.size.width, self.window.size.height) / 2.0
                });
                self.gallery.menu = Some((kind, at));
                self.show_screen();
            }
            Press::GalleryQuery(query) => {
                self.gallery.query = query;
                self.show_screen();
            }
            Press::Menu(crate::manual::menus::MENU, row) => self.press_table_menu(row),
            Press::Log => self.toggle_log(),
            Press::Result(shown) => self.show_result(shown),
            Press::Undo => self.undo(),
            Press::Menu(..) => {}
            Press::Dismiss(Dismiss::Menu) => {
                if self.gallery.menu.take().is_some() {
                    self.show_screen();
                }
                self.close_table_menu();
            }
            Press::Command(index, shift) => self.table_key(index, shift),
            Press::Lobby(press) => self.press_lobby(press),
            Press::Table(press) => self.table_press(press),
        }
    }
}
