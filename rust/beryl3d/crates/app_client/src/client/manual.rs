//! The manual table's side of the client (docs/manual-multiplayer.md): the lobby a solo table
//! opens in (`lobby`), the context menu a right click or a touch held still opens on what the
//! pointer is on, the table's keys, giving a command, and leaving the table. Each command
//! reaches the session as `SessionCommand::TableAct`, and the table's next view comes back as
//! `Input::Update`, as a game's does. What the table shows over itself is `manual_overlays`.

use beryl_protocol::GameUpdate;
use game_hud::inspect::Viewed;
use glam::Vec2;
use kit_ui::element::Action;
use kit_ui::layout::Shake;

use super::{Client, Platform};
use crate::command::SessionCommand;

use app_screens::screens::Screen;
use beryl_protocol::ipc::TableActReply;
use beryl_protocol::protocol::{Answer, LinkState, ObjectId, Side, TableAction, ZoneName};
use game_manual::features::{context_menu, lobby, log, table::Message as TableMessage};
use game_manual::manual::menus::{self, Act};
use game_manual::manual::tokens::TokenChoice;
use game_manual::manual::viewers::{self, Look, ViewerControl};
use game_manual::manual::{self, Context, Pending, Pressed, Sent, Target, commands, pools};
use kit_ui::input::{Button, Pointer, PointerPhase};

/// The most table actions the client remembers sending without an answer.
const PENDING: usize = 64;

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
        // A connection that is up again ends the failure it reported, as the web's
        // `useTableLink` clears its error on a connected link.
        if (update.view.table.as_ref())
            .is_some_and(|table| matches!(table.link, LinkState::Connected { .. }))
        {
            self.update_lobby(&lobby::Message::LinkError(None));
        }
        game_manual::manual::selection::keep(
            &mut self.features.table_mut().selection,
            &update.view,
        );
        if (self.features.table().picking).is_some_and(|pick| {
            !game_manual::manual::selection::actionable(&update.view, pick.from)
        }) {
            self.features.table_mut().picking = None;
        }
        self.features.clear_menu();
        self.features.close_overlay();
        if matches!(self.screen, Screen::Start | Screen::Game) {
            self.screen = Screen::Table;
        }
    }

    /// A right click on the table screen, with no sheet over it, removes one from the mana or
    /// player counter under it, or else opens the context menu of what the pointer is on where
    /// it was clicked (`open_table_menu_at`). Returns whether it did either, and so means
    /// nothing else.
    pub(super) fn open_table_menu(&mut self, pointer: &Pointer) -> bool {
        if pointer.phase != PointerPhase::Down || pointer.button != Button::Secondary {
            return false;
        }
        if self.screen == Screen::Table
            && let Some((_, action, _)) = self
                .layer
                .press_at(pointer.position, self.features.layer_open)
            && let Some(&TableMessage::Counter { side, at }) = action.downcast_ref()
        {
            self.adjust_counter(side, at, false, TableMessage::Counter { side, at });
            return true;
        }
        self.open_table_menu_at(pointer.position)
    }

    /// Adds one to the counter at `at` of `side`'s row of mana and player counters, or removes
    /// one unless `add` (`manual::pools`), unless the table is frozen. `pressed` is the button
    /// that was pressed, which shakes if the table refuses.
    pub(super) fn adjust_counter(&mut self, side: Side, at: u8, add: bool, pressed: TableMessage) {
        if self.table_frozen() {
            return;
        }
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        let Some(name) = pools::counter_name(view, side, at) else {
            return;
        };
        let Some((_, command)) = commands::command(pools::command(at, add)) else {
            return;
        };
        let mut values = commands::Values::new();
        values.insert("text", name.into());
        let actions = commands::actions_with(command, Target::Life(side), view, &values);
        self.features.table_mut().clicked = Some(Pressed::Button(pressed));
        self.send_table(actions);
        self.features.table_mut().clicked = None;
    }

    /// A click on the card in `slot` on the table, one that picked nothing up, as the web's
    /// `registerClicks` (`ui/src/table/click.ts`) takes it: a permanent taps or untaps, with
    /// every selected card if it is selected; a card in the viewer's hand, or in an open viewer
    /// of a library, is played (face down with Shift); a stack entry resolves; and a library's
    /// top card draws a card for its player. With Cmd held, a click adds the card to the
    /// selection or takes it out instead (`table_select`). Returns whether the click was on
    /// the manual table, and so means nothing else.
    pub(super) fn table_click(&mut self, slot: ObjectId) -> bool {
        if self.screen != Screen::Table {
            return false;
        }
        if self.features.table().modifiers.meta {
            self.table_select(slot);
            return true;
        }
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return true;
        };
        let Some(object) = view.objects.iter().find(|object| object.slot == slot) else {
            return true;
        };
        let shift = self.features.table().modifiers.shift;
        let viewed = self.features.viewer().open().is_some() && self.viewer_shows(slot);
        let card = Target::Card(slot);
        let side = object.zone_side.unwrap_or(object.owner);
        let (id, target) = match object.zone {
            ZoneName::Battlefield => ("permanent.tap", card),
            ZoneName::Stack => ("stack_entry.resolve", card),
            ZoneName::Hand if side == Side::You && shift => ("hand_card.play_face_down", card),
            ZoneName::Hand if side == Side::You => ("hand_card.play", card),
            ZoneName::Library if viewed && shift => ("hand_card.play_face_down", card),
            ZoneName::Library if viewed => ("pile_card.play", card),
            ZoneName::Library => ("library.draw", Target::Pile(side, ZoneName::Library)),
            _ => return true,
        };
        self.features.table_mut().clicked = Some(Pressed::Card(slot));
        self.run_table_command(id, target);
        self.features.table_mut().clicked = None;
        true
    }

    /// Opens the context menu of what is at `at` (`manual::target_at`) there, on the table
    /// screen with no sheet over it. Returns whether it did.
    pub(super) fn open_table_menu_at(&mut self, at: Vec2) -> bool {
        if self.screen != Screen::Table || self.features.sheet.is_some() || self.table_frozen() {
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
        self.update_table_menu(&context_menu::Message::Open(menu));
        self.show_screen();
        true
    }

    pub(super) fn close_table_menu(&mut self) {
        if self.update_table_menu(&context_menu::Message::Close) {
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
        let pointed = (self.mouse).map_or(Target::Table, |at| {
            (self.viewer_card_at(at)).unwrap_or_else(|| manual::target_at(&self.frame, at))
        });
        let on_pointer = command.id.starts_with("life.")
            || matches!(command.id, "selection.row" | "selection.column");
        let current = match self.features.table().selection.first() {
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
        // The Quit key asks first, as the web's `TableCommands` sends it to `quit`; Leave
        // table in a menu leaves at once.
        if command.id == "table.leave" {
            self.features.clear_menu();
            self.features.close_overlay();
            self.features.table_mut().quitting = true;
            self.show_screen();
            return;
        }
        let act = menus::act(command, target, view, &self.table_context(), shift);
        self.give(act, target, shift);
    }

    /// Gives the table command whose id is `id` to `target`, as a key or a menu row would,
    /// unless the table is frozen while the other seat's connection is down.
    pub(super) fn run_table_command(&mut self, id: &str, target: Target) {
        if self.table_frozen() {
            return;
        }
        let Some((_, command)) = commands::command(id) else {
            return;
        };
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        if view.table.is_none() {
            return;
        }
        let act = menus::act(command, target, view, &self.table_context(), false);
        self.give(act, target, false);
    }

    /// What the client knows beyond the table's view that decides which commands it offers:
    /// the platform, the keys the player bound and the last token made.
    pub(super) fn table_context(&self) -> Context<'_> {
        Context {
            mac: self.platform == Platform::Mac,
            bindings: &self.settings.bindings,
            last_token: self.features.table().last_token.as_ref(),
            selected: &self.features.table().selection,
            viewer_open: self.features.viewer().open().is_some(),
            whole_library: self.whole_library_open(),
        }
    }

    /// Sends `actions` to the table, in order, keeping for each what its answer asks of the
    /// client (`Sent`). Sending an undo or a rewind stops highlighting the lines that blocked
    /// the last one, as the web's `sendHistory` clears them before it sends.
    pub(super) fn send_table(&mut self, actions: Vec<TableAction>) {
        for action in actions {
            let look = (self.show.latest()).and_then(|update| Look::of(&action, &update.view));
            let sent = match (&action, TokenChoice::of(&action), look) {
                (_, Some(token), _) => Sent::Token(token),
                (_, None, Some(look)) => Sent::Look(look),
                (TableAction::Undo { .. } | TableAction::Rewind { .. }, None, None) => {
                    self.update_table_log(&log::Message::ClearBlocked);
                    Sent::History
                }
                _ => Sent::Other,
            };
            let from = self.features.table().clicked;
            self.remember(Pending { sent, from });
            self.commands.push_back(SessionCommand::TableAct(action));
        }
    }

    /// Sends `answer` to the table's prompt or assist. Its reply comes back as a table action's
    /// does, so it waits in the same queue, asking nothing of the reply but the refusal sound.
    pub(super) fn send_answer(&mut self, answer: Answer) {
        self.remember(Pending {
            sent: Sent::Other,
            from: None,
        });
        self.commands.push_back(SessionCommand::TableAnswer(answer));
    }

    /// Remembers `pending` until the table answers it.
    fn remember(&mut self, pending: Pending) {
        // A driver that never answers, such as a recording's, would otherwise make the list
        // grow for as long as the table is open.
        if self.features.table().pending.len() >= PENDING {
            self.features.table_mut().pending.pop_front();
        }
        self.features.table_mut().pending.push_back(pending);
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
            && (self.show.latest())
                .is_some_and(|update| game_manual::manual::link::frozen(&update.view))
    }

    /// Advances the shake of a refused button to the frame at `now` and moves the flat layer's
    /// elements that take its press. Returns whether it is still shaking.
    pub(super) fn shake_button(&mut self, now: u64) -> bool {
        if self.features.table().shaking.is_none() {
            return false;
        }
        let Some((button, shake)) = &mut self.features.table_mut().shaking else {
            return false;
        };
        if !shake.advance(now) {
            self.features.table_mut().shaking = None;
            self.layer.shake(None);
            return false;
        }
        self.layer.shake(Some((button, shake.offset(now))));
        true
    }

    /// The table answered the oldest table action not yet answered: a token it made is the one
    /// Create another token makes again, and an undo or rewind it refused because later lines
    /// depend on it opens the log with those lines highlighted. Any refusal plays the refusal
    /// sound in the next frame, so that a key or a drop the table would not take does not
    /// pass in silence, and shakes the card or button clicked to send the action, if one was, unless
    /// the table froze since (`shakeRefusal` in `ui/src/stage/Refusal.tsx`).
    pub(super) fn take_table_reply(&mut self, reply: TableActReply) {
        let refused = matches!(reply, TableActReply::Refused(_));
        if refused {
            self.features.table_mut().refused = true;
        }
        let Some(Pending { sent, from }) = self.features.table_mut().pending.pop_front() else {
            return;
        };
        let refusal_ms = self.data().pointer.refusal_ms;
        match from.filter(|_| refused && !self.table_frozen()) {
            Some(Pressed::Card(slot)) => self
                .show
                .pointer_mut()
                .refuse(usize::from(slot.0), refusal_ms),
            Some(Pressed::Button(button)) => {
                self.features.table_mut().shaking =
                    Some((Action::new(button), Shake::new(refusal_ms)));
            }
            None => {}
        }
        match (sent, reply) {
            (Sent::Token(token), TableActReply::Ok { .. }) => {
                self.features.table_mut().last_token = Some(token);
            }
            (Sent::Look(look), TableActReply::Ok { .. }) => {
                self.open_look(look);
                return;
            }
            (Sent::History, TableActReply::Refused(refused)) => {
                self.update_table_log(&log::Message::Blocked(
                    refused.blocked_by.unwrap_or_default(),
                ));
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
            Act::Viewer(control) => self.viewer_control(control),
            Act::Pick(index) => self.start_pick(index, target),
            Act::Answer(answer) => self.send_answer(answer),
            Act::AskX(source, row) => {
                self.show_ask(game_manual::manual::ask::Ask::ability_x(
                    source, row, target,
                ));
            }
            Act::Leave => self.leave_table(),
            Act::Nothing => {}
        }
    }

    /// Leaves the table, from the table or its lobby, and shows the start screen.
    pub(super) fn leave_table(&mut self) {
        self.features.clear_menu();
        self.features.close_overlay();
        self.features.table_mut().selection.clear();
        self.features.table_mut().picking = None;
        self.features.table_mut().mana_sheet = None;
        self.update_lobby(&lobby::Message::Reset);
        self.features.lobby_view = None;
        self.commands.push_back(SessionCommand::TableLeave);
        self.screen = Screen::Start;
        self.change_sheet(None);
        self.show_screen();
    }
}

impl Client {
    /// The table took the look `look` at a library: its viewer opens, in place of any viewer
    /// open, as the web's `TableViewers.look` opens it once the table answers.
    fn open_look(&mut self, look: Look) {
        let side = look.side;
        self.view(Some(Viewed::Pile(side, ZoneName::Library)));
        self.features.table_mut().viewer.look = Some(look);
        self.show_screen();
    }

    /// Whether the open viewer shows a search of a whole library its player may shuffle.
    fn whole_library_open(&self) -> bool {
        (self.show.latest())
            .is_some_and(|update| self.features.table().viewer.can_shuffle(&update.view))
    }

    /// Does what a client command does to the open viewer (`ViewerControl`). With no viewer
    /// open it does nothing.
    pub(super) fn viewer_control(&mut self, control: ViewerControl) {
        if self.features.viewer().open().is_none() {
            return;
        }
        match control {
            ViewerControl::Close => self.dismiss_viewer(),
            ViewerControl::Search => self.layer.focus_field(viewers::SEARCH),
            ViewerControl::Group => {
                self.features.table_mut().viewer.grouped = !self.features.table().viewer.grouped;
                self.show_screen();
            }
            ViewerControl::Sort(sort) => {
                self.features.table_mut().viewer.sort = sort;
                self.show_screen();
            }
            ViewerControl::Finish { shuffle } => {
                if self.whole_library_open() {
                    self.close_library(shuffle);
                }
            }
        }
    }

    /// The viewer's close button or Escape: on the manual table, closing a search of a whole
    /// library its player may shuffle first asks whether to shuffle it, as the web's
    /// `TablePileViewer` does, and closing a library's viewer ends the look. Any other viewer
    /// closes.
    pub(super) fn dismiss_viewer(&mut self) {
        if self.screen != Screen::Table || self.features.table().viewer.look.is_none() {
            self.view(None);
        } else if self.whole_library_open() && !self.features.table().viewer.confirming {
            self.features.table_mut().viewer.confirming = true;
            self.show_screen();
        } else {
            self.close_library(false);
        }
    }

    /// Ends the look the open viewer shows, shuffling the whole library first if `shuffle`
    /// and its player may, in one batch as the web's `TableViewers.close` sends it, and closes
    /// the viewer.
    fn close_library(&mut self, shuffle: bool) {
        let shuffled = shuffle && self.whole_library_open();
        let Some(look) = self.features.table_mut().viewer.look.take() else {
            return;
        };
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        let action = look.close(view, shuffled);
        self.send_table(vec![action]);
        self.view(None);
    }
}
