//! The manual table's side of the client (docs/manual-multiplayer.md): the lobby a solo table
//! opens in (`lobby`), the context menu a right click or a touch held still opens on what the
//! pointer is on, the table's keys, giving a command, and leaving the table. Each command
//! reaches the session as `SessionCommand::TableAct`, and the table's next view comes back as
//! `Input::Update`, as a game's does. What the table shows over itself is `manual_overlays`.

use beryl_protocol::GameUpdate;
use glam::Vec2;

use super::{Client, Platform};
use crate::command::SessionCommand;
use crate::input::{Button, Pointer, PointerPhase};
use crate::manual::lobby::Lobby;
use crate::manual::menus::{self, Act};
use crate::manual::tokens::TokenChoice;
use crate::manual::{self, Context, Sent, TableState, Target, commands};
use crate::screens::Screen;
use crate::widgets::BACK;
use beryl_protocol::ipc::TableActReply;
use beryl_protocol::protocol::{Answer, TableAction};

/// The most table actions the client remembers sending without an answer.
const PENDING: usize = 64;

/// What the client keeps of the manual table: what the table shows over itself
/// (`TableState`, which `Manual` derefs to), the keys held at the last press, and the lobby.
#[derive(Debug, Default)]
pub(super) struct Manual {
    pub(super) state: TableState,
    /// Whether Cmd (Control off the Mac) was held at the last press of the pointer, which
    /// makes a click on a card add it to the selection or take it out.
    pub(super) meta: bool,
    pub(super) lobby: Lobby,
    /// Whether the table refused an action, or the open decision a click on a card, since the
    /// last frame, which then plays the refusal sound, as the web's `shakeRefusal` plays it for
    /// every refused table action and `useRefusal` for every refused click.
    pub(super) refused: bool,
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

impl Manual {
    /// The refusal sound, once for a refusal since the last frame, as the web's
    /// `shakeRefusal` plays it.
    pub(super) fn take_refusal(&mut self) -> Option<crate::frame::Sound> {
        std::mem::take(&mut self.refused).then(|| crate::frame::Sound {
            id: crate::sound::id("no"),
            theirs: false,
        })
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
        crate::manual::selection::keep(&mut self.manual.state.selection, &update.view);
        if (self.manual.picking)
            .is_some_and(|pick| !crate::manual::selection::actionable(&update.view, pick.from))
        {
            self.manual.picking = None;
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

    /// Opens the context menu of what is at `at` (`manual::target_at`) there, on the table
    /// screen with no sheet over it. Returns whether it did.
    pub(super) fn open_table_menu_at(&mut self, at: Vec2) -> bool {
        if self.screen != Screen::Table || self.sheet.is_some() || self.table_frozen() {
            return false;
        }
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return false;
        };
        if view.table.is_none() {
            return false;
        }
        let target =
            (self.viewer_card_at(at)).unwrap_or_else(|| manual::target_at(&self.frame, at));
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

    /// A key of the table gave the command at `index` of `COMMANDS`. It acts as the web's keys
    /// do (`listenTableKeys` in `ui/src/table/keyboard.ts` and `TableCommands.shortcut` in
    /// `ui/src/table/TableMenu.tsx`): on the first selected card while cards are selected, and
    /// otherwise on what the mouse is over (a card of an open pile viewer before the table
    /// under it), except that a life key and the row and column selections always act on what
    /// the mouse is over. A command that does not apply there acts on the table or on the
    /// viewer's own library, hand, life total, graveyard or exile, the first it applies to, so
    /// that Cmd+D over the opponent's life total draws the viewer a card, not the opponent.
    /// The viewer's player and sideboard have no `Target`; the table stands for them. With
    /// `shift`, a token command makes its token face down.
    pub(super) fn table_key(&mut self, index: u16, shift: bool) {
        use beryl_protocol::protocol::{Side, ZoneName};
        let Some(command) = commands::COMMANDS.get(usize::from(index)) else {
            return;
        };
        if self.table_frozen() {
            return;
        }
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        if view.table.is_none() {
            return;
        }
        let pointed = (self.owned.mouse).map_or(Target::Table, |at| {
            (self.viewer_card_at(at)).unwrap_or_else(|| manual::target_at(&self.frame, at))
        });
        let on_pointer = command.id.starts_with("life.")
            || matches!(command.id, "selection.row" | "selection.column");
        let current = match self.manual.state.selection.first() {
            Some(&slot) if !on_pointer => Target::Card(slot),
            _ => pointed,
        };
        let fallbacks = [
            Target::Table,
            Target::Pile(Side::You, ZoneName::Library),
            Target::Hand(Side::You),
            Target::Life(Side::You),
            Target::Pile(Side::You, ZoneName::Graveyard),
            Target::Pile(Side::You, ZoneName::Exile),
        ];
        let target = std::iter::once(current)
            .chain(fallbacks)
            .find(|target| command.applies(target.applies_to(view)))
            .unwrap_or(Target::Table);
        let act = menus::act(command, target, view, &self.table_context(), shift);
        self.give(act, target, shift);
    }

    /// What the client knows beyond the table's view that decides which commands it offers:
    /// the platform, the keys the player bound and the last token made.
    pub(super) fn table_context(&self) -> Context<'_> {
        Context {
            mac: self.platform == Platform::Mac,
            bindings: &self.settings.bindings,
            last_token: self.manual.last_token.as_ref(),
            selected: &self.manual.state.selection,
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

    /// Sends `answer` to the table's prompt or assist. Its reply comes back as a table action's
    /// does, so it waits in the same queue, asking nothing of the reply but the refusal sound.
    pub(super) fn send_answer(&mut self, answer: Answer) {
        if self.manual.pending.len() >= PENDING {
            self.manual.pending.pop_front();
        }
        self.manual.pending.push_back(Sent::Other);
        self.commands.push_back(SessionCommand::TableAnswer(answer));
    }

    /// Whether the table on screen shows the guided start's opening (`manual::opening`).
    pub(super) fn guided_start(&self) -> bool {
        self.screen == Screen::Table
            && (self.show.latest())
                .is_some_and(|update| manual::opening::prompt(&update.view).is_some())
    }

    /// Whether the table on screen is frozen because the other seat's connection is down or
    /// the other player left (`manual::link`): the pointer and the keys then reach only the
    /// banner's Lobby and what the flat layer shows over every screen.
    pub(super) fn table_frozen(&self) -> bool {
        self.screen == Screen::Table
            && (self.show.latest()).is_some_and(|update| crate::manual::link::frozen(&update.view))
    }

    /// The table answered the oldest table action not yet answered: a token it made is the one
    /// Create another token makes again, and an undo or rewind it refused because later lines
    /// depend on it opens the log with those lines highlighted. Any refusal plays the refusal
    /// sound in the next frame, so that a key or a drop the table would not take does not
    /// pass in silence.
    pub(super) fn take_table_reply(&mut self, reply: TableActReply) {
        if matches!(reply, TableActReply::Refused(_)) {
            self.manual.refused = true;
        }
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
            Act::View(viewed) => self.view(Some(viewed)),
            Act::Pick(index) => self.start_pick(index, target),
            Act::Answer(answer) => self.send_answer(answer),
            Act::AskX(source, row) => {
                self.show_ask(crate::manual::ask::Ask::ability_x(source, row, target));
            }
            Act::Leave => self.leave_table(),
            Act::Nothing => {}
        }
    }

    /// Leaves the table, from the table or its lobby, and shows the start screen.
    pub(super) fn leave_table(&mut self) {
        self.manual.menu = None;
        self.manual.close_overlay();
        self.manual.state.selection.clear();
        self.manual.picking = None;
        self.manual.lobby = Lobby::new();
        self.commands.push_back(SessionCommand::TableLeave);
        self.screen = Screen::Start;
        self.sheet = None;
        self.show_screen();
    }
}
