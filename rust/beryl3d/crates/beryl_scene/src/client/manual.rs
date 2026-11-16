//! The manual table's side of the client (docs/manual-multiplayer.md): the lobby a solo table
//! opens in (`lobby`), the context menu a right click or a touch held still opens on what the
//! pointer is on, the table's keys, giving a command, and leaving the table. Each command
//! reaches the session as `SessionCommand::TableAct`, and the table's next view comes back as
//! `Input::Update`, as a game's does. What the table shows over itself is `manual_overlays`.

use beryl_protocol::GameUpdate;
use glam::Vec2;

use super::{Client, Platform};
use crate::command::SessionCommand;
use crate::data::space::pointer::{DRAG_THRESHOLD_POINTS, LONG_PRESS_MS};
use crate::input::{Button, Pointer, PointerKind, PointerPhase};
use crate::manual::lobby::Lobby;
use crate::manual::menus::{self, Act};
use crate::manual::tokens::TokenChoice;
use crate::manual::{self, Context, Sent, TableState, Target, commands};
use crate::screens::Screen;
use crate::widgets::BACK;
use beryl_protocol::ipc::TableActReply;
use beryl_protocol::protocol::TableAction;

/// Nanoseconds in a millisecond, the unit of `Client::frame`'s clock.
const MILLISECOND: u64 = 1_000_000;

/// The most table actions the client remembers sending without an answer.
const PENDING: usize = 64;

/// What the client keeps of the manual table: what the table shows over itself
/// (`TableState`, which `Manual` derefs to), the touch that will open a context menu if it
/// stays down and still, and the lobby.
#[derive(Debug, Default)]
pub(super) struct Manual {
    pub(super) state: TableState,
    hold: Option<Hold>,
    pub(super) lobby: Lobby,
}

impl std::ops::Deref for Manual {
    type Target = TableState;

    fn deref(&self) -> &TableState {
        &self.state
    }
}

impl std::ops::DerefMut for Manual {
    fn deref_mut(&mut self) -> &mut TableState {
        &mut self.state
    }
}

/// A touch that went down on the table and has not moved `DRAG_THRESHOLD_POINTS` from where it
/// went down. Held for `LONG_PRESS_MS`, it opens the context menu there, as the web's
/// `Pointer` does on a long press (`LONG_PRESS_DELAY` in `ui/src/stage/pointer.ts`).
#[derive(Clone, Copy, Debug)]
struct Hold {
    finger: u8,
    at: Vec2,
    /// The `now` of the first frame after the touch went down, from which the hold is timed:
    /// an input has no time of its own.
    since: Option<u64>,
}

impl Manual {
    /// When the touch held still opens its menu, if one is down and its timing has begun.
    pub(super) fn hold_due(&self) -> Option<u64> {
        let since = self.hold?.since?;
        // The hold time is a whole number of milliseconds in `space.json`.
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let hold = LONG_PRESS_MS as u64 * MILLISECOND;
        Some(since + hold)
    }
}

impl Client {
    /// An update arrived: it keeps the session it came from, which stop and concede commands
    /// name. A view of a manual table shows the table screen, from the start screen or a game,
    /// and closes a context menu opened on the view before it, whose rows name that view's
    /// cards. So does a command list, ask prompt, token search or order tray, as the web's
    /// `TableMenu` closes its overlay when the view changes. In the lobby it stays there: a
    /// table sends its view while it is still in its lobby, and the lobby says when its game has
    /// started (`take_lobby`).
    pub(super) fn take_session(&mut self, update: &GameUpdate) {
        self.session = Some(update.session);
        if update.view.table.is_none() {
            return;
        }
        self.manual.menu = None;
        self.manual.close_overlay();
        if matches!(self.screen, Screen::Start | Screen::Game) {
            self.screen = Screen::Table;
        }
    }

    /// A right click on the table screen, with no sheet over it, opens the context menu of
    /// what the pointer is on where it was clicked (`open_table_menu_at`). Returns whether it
    /// did, and so means nothing else.
    pub(super) fn open_table_menu(&mut self, pointer: &Pointer) -> bool {
        pointer.phase == PointerPhase::Down
            && pointer.button == Button::Secondary
            && self.open_table_menu_at(pointer.position)
    }

    /// Follows a touch on the table screen that may become a long press: one going down with
    /// no menu or sheet open starts a `Hold`, and moving `DRAG_THRESHOLD_POINTS` from where it
    /// went down, lifting or being cancelled ends it. `hold_table_menu` opens the menu once
    /// it has been held long enough.
    pub(super) fn follow_hold(&mut self, pointer: &Pointer) {
        if pointer.kind != PointerKind::Touch {
            return;
        }
        match pointer.phase {
            PointerPhase::Down => {
                let table = self.screen == Screen::Table
                    && self.sheet.is_none()
                    && self.manual.menu.is_none()
                    && (self.show.latest()).is_some_and(|update| update.view.table.is_some());
                self.manual.hold = table.then_some(Hold {
                    finger: pointer.id,
                    at: pointer.position,
                    since: None,
                });
            }
            PointerPhase::Move => {
                if let Some(hold) = self.manual.hold
                    && hold.finger == pointer.id
                    && hold.at.distance(pointer.position) >= DRAG_THRESHOLD_POINTS
                {
                    self.manual.hold = None;
                }
            }
            PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave => {
                if self
                    .manual
                    .hold
                    .is_some_and(|hold| hold.finger == pointer.id)
                {
                    self.manual.hold = None;
                }
            }
            PointerPhase::Wheel => {}
        }
    }

    /// At the frame at `now`, a touch held still since `LONG_PRESS_MS` ago opens the context
    /// menu where it went down. The touch's press ends there: lifting the finger clicks no
    /// card, opens no pile and presses no row of the menu that opened under it.
    pub(super) fn hold_table_menu(&mut self, now: u64) {
        let Some(hold) = &mut self.manual.hold else {
            return;
        };
        let at = hold.at;
        hold.since.get_or_insert(now);
        if self.manual.hold_due().is_none_or(|due| now < due) {
            return;
        }
        self.manual.hold = None;
        if self.open_table_menu_at(at) {
            self.show.pointer_mut().cancel();
            self.pressed = None;
            self.viewing.down = None;
        }
    }

    /// Opens the context menu of what is at `at` (`manual::target_at`) there, on the table
    /// screen with no sheet over it. Returns whether it did.
    fn open_table_menu_at(&mut self, at: Vec2) -> bool {
        if self.screen != Screen::Table || self.sheet.is_some() {
            return false;
        }
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return false;
        };
        if view.table.is_none() {
            return false;
        }
        let target = manual::target_at(&self.frame, at);
        let menu = menus::open(target, view, at, &self.table_context());
        self.manual.menu = Some(menu);
        self.show_screen();
        true
    }

    /// The `row`th row of the open context menu was pressed: it closes, and the row's command
    /// is given to what the menu was opened on. On the phone's sheet, a row with a submenu
    /// instead shows that submenu's rows in the sheet, and `BACK` shows the menu's rows again, as
    /// the web's menu pages do.
    pub(super) fn press_table_menu(&mut self, row: u16) {
        let phone = self.window.phone();
        let Some(menu) = &mut self.manual.menu else {
            return;
        };
        let folder = (menu.groups.iter().flat_map(|group| &group.rows))
            .nth(usize::from(row))
            .is_some_and(|row| !row.submenu.is_empty());
        if phone && (row == BACK || folder) {
            menu.page = (row != BACK).then_some(row);
            self.show_screen();
            return;
        }
        if let Some(menu) = self.manual.menu.take()
            && let Some(act) = menu.acts.into_iter().nth(usize::from(row))
        {
            self.give(act, menu.target, false);
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
        let act = menus::act(command, target, view, &self.table_context());
        self.give(act, target, false);
    }

    /// What the client knows beyond the table's view that decides which commands it offers:
    /// the platform, the keys the player bound and the last token made.
    pub(super) fn table_context(&self) -> Context<'_> {
        Context {
            mac: self.platform == Platform::Mac,
            bindings: &self.settings.bindings,
            last_token: self.manual.last_token.as_ref(),
        }
    }

    /// Sends `actions` to the table, in order, keeping for each what its answer asks of the
    /// client (`Sent`). Sending an undo or a rewind stops highlighting the lines that blocked
    /// the last one, as the web's `sendHistory` clears them before it sends.
    pub(super) fn send_table(&mut self, actions: Vec<TableAction>) {
        for action in actions {
            let sent = match (&action, TokenChoice::of(&action)) {
                (_, Some(token)) => Sent::Token(token),
                (TableAction::Undo { .. } | TableAction::Rewind { .. }, None) => {
                    self.manual.blocked.clear();
                    Sent::History
                }
                _ => Sent::Other,
            };
            // A driver that never answers, such as a recording's, would otherwise make the
            // list grow for as long as the table is open.
            if self.manual.pending.len() >= PENDING {
                self.manual.pending.pop_front();
            }
            self.manual.pending.push_back(sent);
            self.commands.push_back(SessionCommand::TableAct(action));
        }
    }

    /// The table answered the oldest table action not yet answered: a token it made is the one
    /// Create another token makes again, and an undo or rewind it refused because later lines
    /// depend on it opens the log with those lines highlighted.
    pub(super) fn take_table_reply(&mut self, reply: TableActReply) {
        let Some(sent) = self.manual.pending.pop_front() else {
            return;
        };
        match (sent, reply) {
            (Sent::Token(token), TableActReply::Ok { .. }) => {
                self.manual.last_token = Some(token);
            }
            (Sent::History, TableActReply::Refused(refused)) => {
                let blocked = refused.blocked_by.unwrap_or_default();
                if !blocked.is_empty() {
                    self.manual.blocked = blocked;
                    self.manual.log_open = true;
                }
            }
            _ => return,
        }
        self.show_screen();
    }

    /// Does what a command does for `target`: sends its table actions; opens the ask prompt,
    /// the token search (whose token is made face down if `shift`) or the command list, with
    /// the row that resets the keys for Shortcuts; or leaves the table and shows the start
    /// screen.
    pub(super) fn give(&mut self, act: Act, target: Target, shift: bool) {
        match act {
            Act::Actions(actions) => self.send_table(actions),
            Act::Ask(index) => self.open_ask(index, target),
            Act::Tokens(index) => self.open_tokens(index, shift),
            Act::Palette => self.open_palette(target, false),
            Act::Shortcuts => self.open_palette(target, true),
            Act::Leave => self.leave_table(),
            Act::Nothing => {}
        }
    }

    /// Leaves the table, from the table or its lobby, and shows the start screen.
    pub(super) fn leave_table(&mut self) {
        self.manual.menu = None;
        self.manual.close_overlay();
        self.manual.lobby = Lobby::new();
        self.commands.push_back(SessionCommand::TableLeave);
        self.screen = Screen::Start;
        self.sheet = None;
        self.show_screen();
    }
}
