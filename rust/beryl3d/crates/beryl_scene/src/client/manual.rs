//! The manual table's side of the client (docs/manual-multiplayer.md): starting a solo table
//! from the start screen, the context menu a right click opens on what the pointer is on, the
//! table's keys, and leaving the table. Each command reaches the session as
//! `SessionCommand::TableAct`, and the table's next view comes back as `Input::Update`, as a
//! game's does.

use beryl_protocol::GameUpdate;

use super::{Client, Platform};
use crate::command::SessionCommand;
use crate::input::{Button, Pointer, PointerPhase};
use crate::manual::menus::{self, Act, OpenMenu};
use crate::manual::{self, Target, commands};
use crate::screens::Screen;

/// What the client keeps of the manual table: the open context menu, if any.
#[derive(Debug, Default)]
pub(super) struct Manual {
    pub(super) menu: Option<OpenMenu>,
}

impl Client {
    /// The start screen's 'Solo table': starts a solo table with the sample deck and shows the
    /// table, where its first view arrives.
    pub(super) fn start_table(&mut self) {
        (self.commands).push_back(SessionCommand::TableSolo(manual::solo_request()));
        self.show_table();
    }

    /// An update arrived: it keeps the session it came from, which stop and concede commands
    /// name. A view of a manual table shows the table screen, from the start screen or a game,
    /// and closes a context menu opened on the view before it, whose rows name that view's
    /// cards.
    pub(super) fn take_session(&mut self, update: &GameUpdate) {
        self.session = Some(update.session);
        if update.view.table.is_none() {
            return;
        }
        self.manual.menu = None;
        if matches!(self.screen, Screen::Start | Screen::Game) {
            self.screen = Screen::Table;
        }
    }

    /// A right click on the table screen, with no sheet over it, opens the context menu of
    /// what the pointer is on (`manual::target_at`) where it was clicked. Returns whether it
    /// did, and so means nothing else.
    pub(super) fn open_table_menu(&mut self, pointer: &Pointer) -> bool {
        if self.screen != Screen::Table
            || self.sheet.is_some()
            || pointer.phase != PointerPhase::Down
            || pointer.button != Button::Secondary
        {
            return false;
        }
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return false;
        };
        if view.table.is_none() {
            return false;
        }
        let target = manual::target_at(&self.frame, pointer.position);
        let mac = self.platform == Platform::Mac;
        self.manual.menu = Some(menus::open(target, view, pointer.position, mac));
        self.show_screen();
        true
    }

    /// The `row`th row of the open context menu was pressed: it closes, and the row's command
    /// is given.
    pub(super) fn press_table_menu(&mut self, row: u16) {
        let Some(menu) = self.manual.menu.take() else {
            return;
        };
        if let Some(act) = menu.acts.into_iter().nth(usize::from(row)) {
            self.give(act);
        }
        self.show_screen();
    }

    /// Closes the open context menu.
    pub(super) fn close_table_menu(&mut self) {
        if self.manual.menu.take().is_some() {
            self.show_screen();
        }
    }

    /// A key of the table gave the command at `index` of `COMMANDS`: it acts on what the mouse
    /// is over, or on the table with no mouse, as the web's keys act on the pointed target.
    pub(super) fn table_key(&mut self, index: u16) {
        let Some(command) = commands::COMMANDS.get(usize::from(index)) else {
            return;
        };
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        if view.table.is_none() {
            return;
        }
        let target =
            (self.owned.mouse).map_or(Target::Table, |at| manual::target_at(&self.frame, at));
        let act = menus::act(command, target, view);
        self.give(act);
    }

    /// Does what a command does: sends its table actions, or leaves the table and shows the
    /// start screen.
    fn give(&mut self, act: Act) {
        match act {
            Act::Actions(actions) => {
                (self.commands).extend(actions.into_iter().map(SessionCommand::TableAct));
            }
            Act::Leave => {
                self.manual.menu = None;
                self.commands.push_back(SessionCommand::TableLeave);
                self.screen = Screen::Start;
                self.sheet = None;
                self.show_screen();
            }
            Act::Nothing => {}
        }
    }

    /// Shows the table screen.
    fn show_table(&mut self) {
        self.first_view_opens = None;
        self.screen = Screen::Table;
        self.sheet = None;
        self.show_screen();
    }
}
