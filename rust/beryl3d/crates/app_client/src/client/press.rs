//! Dispatches messages received from the flat layer and the client's own controls.

use super::Client;
use app_screens::screens::ScreenPress;
use game_manual::manual::{LobbyPress, TableCommand, TablePress};
use kit_ui::element::Action;
use kit_ui::layout::DismissMenu;
use kit_ui::widgets::{MenuChoice, MenuId};

/// What a press on an element means. The client turns it into a `SessionCommand`, a
/// `Request` or a change of screen, sheet or settings, adding what a screen function does not
/// know, such as the open decision's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Press {
    /// A row of an open menu, counting from 0 over all its groups.
    Menu(MenuId, u16),
    /// Open one of the gallery's example context menus.
    GalleryMenu(app_screens::screens::GalleryMenu),
    /// Show one of the gallery's example command searches, or close its preview.
    GalleryQuery(Option<app_screens::screens::GalleryQuery>),
    /// Close something shown over the screen.
    Dismiss(Dismiss),
    /// Give the manual table's command at this index of `manual::commands::COMMANDS` to what
    /// the pointer is on, as a key of the table does. The flag is whether Shift made the key's
    /// token face down.
    Command(u16, bool),
    /// Something the manual table shows over itself: its command list, ask prompt, token
    /// search, library order tray or log.
    Table(game_manual::manual::TablePress),
    /// One of the manual table's lobby controls.
    Lobby(LobbyPress),
}

/// What a `Press::Dismiss` closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dismiss {
    /// The open menu.
    Menu,
}

impl Client {
    /// Carries a flat-layer message to the feature that handles it.
    pub fn press(&mut self, action: &Action) {
        if let Some(changed) = self.route_feature(action) {
            if changed {
                self.show_screen();
            }
        } else if let Some(press) = action.downcast_ref::<Press>() {
            self.dispatch_press(press);
        } else if let Some(&press) = action.downcast_ref::<ScreenPress>() {
            let press = match press {
                ScreenPress::GalleryMenu(kind) => Press::GalleryMenu(kind),
                ScreenPress::GalleryQuery(query) => Press::GalleryQuery(query),
                ScreenPress::DismissMenu => Press::Dismiss(Dismiss::Menu),
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
            Press::GalleryMenu(kind) => {
                let at = self.mouse.unwrap_or_else(|| {
                    glam::Vec2::new(self.window.size.width, self.window.size.height) / 2.0
                });
                self.gallery.menu = Some((kind, at));
                self.show_screen();
            }
            Press::GalleryQuery(query) => {
                self.gallery.query = query;
                self.show_screen();
            }
            Press::Menu(game_manual::manual::menus::MENU, row) => self.press_table_menu(row),
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
