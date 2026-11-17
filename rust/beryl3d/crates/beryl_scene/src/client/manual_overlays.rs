//! What the manual table shows over itself, on the client's side (docs/manual-multiplayer.md,
//! "The command list", "Prompts for numbers and text", "The log, undo and rewind"): opening
//! the command list, the ask prompt, the token search and the library order tray, the keys
//! they take while open (Escape closes, Return runs or confirms, the arrows move the
//! highlight), their presses, the store's answers to a token search, the custom token form,
//! binding a key to a command from the command list, and the log's undo and rewind. At most one
//! of these is open at a time; opening one closes the others.

use beryl_protocol::ipc::decks::CardSearchHit;
use beryl_protocol::protocol::{Answer, ObjectId, Side};

use super::Client;
use crate::command::{Request, SessionCommand};
use crate::input::{Key, KeyCode, KeyPhase};
use crate::manual::ask::{self, Ask, Confirmed};
use crate::manual::bindings::binding_for;
use crate::manual::commands::{self, COMMANDS, Values};
use crate::manual::menus;
use crate::manual::order::LibraryOrder;
use crate::manual::palette::{self, Palette, RESET, RESET_NOTICE};
use crate::manual::tokens::{self, Choice, CustomToken, TokenChoice, TokenSearch};
use crate::manual::{TablePress, Target, assists, log};
use crate::widgets::FieldState;

impl Client {
    /// Opens the command list acting on `target`, with its field empty and focused, and with
    /// the row that resets the keys at its top if `settings`.
    pub(super) fn open_palette(&mut self, target: Target, settings: bool) {
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        let palette = Palette::open(target, view, &self.table_context(), settings);
        self.over(palette::FIELD, FieldState::default());
        self.manual.palette = Some(palette);
        self.show_screen();
        self.layer.focus_field(palette::FIELD);
    }

    /// Opens the ask prompt of the command at `index` of `COMMANDS` acting on `target`.
    pub(super) fn open_ask(&mut self, index: u16, target: Target) {
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        if let Some(ask) = Ask::open(index, target, view) {
            self.show_ask(ask);
        }
    }

    /// Opens the token search of the command at `index` of `COMMANDS`, whose token is made face
    /// down if `face_down`.
    pub(super) fn open_tokens(&mut self, index: u16, face_down: bool) {
        self.over(tokens::FIELD, FieldState::default());
        self.manual.tokens = Some(TokenSearch::open(index, face_down));
        self.show_screen();
        self.layer.focus_field(tokens::FIELD);
    }

    /// Opens the library order tray for `cards`, the top card first, going onto `side`'s
    /// library: what putting several cards on a library at once opens.
    pub fn order_library(&mut self, side: Side, cards: Vec<ObjectId>) {
        self.over("", FieldState::default());
        self.manual.order = Some(LibraryOrder::new(side, cards));
        self.show_screen();
    }

    /// Closes whatever the table shows over itself, and starts the field named `field` holding
    /// `state`.
    fn over(&mut self, field: &str, state: FieldState) {
        self.manual.menu = None;
        self.manual.close_overlay();
        self.layer.focus(None);
        if !field.is_empty() {
            self.fields.set(field, state);
        }
    }

    /// Shows `ask` in the dock, its field holding its starting text, selected and focused.
    pub(super) fn show_ask(&mut self, ask: Ask) {
        self.over(&ask.field, ask.field_state());
        let name = ask.field.clone();
        self.manual.ask = Some(ask);
        self.show_screen();
        self.layer.focus_field(&name);
    }

    /// Keeps what the command list's or the token search's field holds as its query, and asks
    /// the store for tokens matching a token search's new query.
    pub(super) fn follow_table_query(&mut self) {
        if let Some(open) = &mut self.manual.palette {
            open.set_query(&self.fields.get(palette::FIELD).text);
        }
        if let Some(open) = &mut self.manual.tokens {
            let query = self.fields.get(tokens::FIELD).text;
            open.set_query(&query);
            if let Some(text) = open.wants(&query) {
                open.asked = Some(text.clone());
                self.commands.push_back(SessionCommand::CardSearch(text));
            }
        }
    }

    /// The store found `hits` for the token search's `text`.
    pub(super) fn take_token_hits(&mut self, text: String, hits: Vec<CardSearchHit>) {
        if let Some(open) = &mut self.manual.tokens {
            open.answer = Some((text, hits));
            self.show_screen();
        }
    }

    /// A press on something the table shows over itself.
    pub(super) fn table_press(&mut self, press: TablePress) {
        let shift = self.manual.shift;
        match press {
            TablePress::Palette(at) => self.run_palette_row(usize::from(at), shift),
            TablePress::AskConfirm => self.confirm_ask(),
            TablePress::AskStep(up) => self.step_ask(up),
            TablePress::AskSuggestion(at) => {
                let suggested = (self.manual.ask.as_ref())
                    .and_then(ask::suggestions)
                    .and_then(|names| names.get(usize::from(at)));
                if let Some(&name) = suggested {
                    self.confirm_ask_with(name);
                }
            }
            TablePress::Token(at) => self.choose_token(usize::from(at), shift),
            TablePress::OrderCard(at) => {
                if let Some(order) = &mut self.manual.order {
                    order.shift(usize::from(at), 0);
                }
                self.show_screen();
            }
            TablePress::OrderDone => self.order_done(),
            TablePress::Undo(entry) => self.send_table(vec![log::undo(entry)]),
            TablePress::Rewind(entry) => self.send_table(vec![log::rewind(entry)]),
            TablePress::ToggleLog => {
                self.manual.log_open = !self.manual.log_open;
                self.show_screen();
            }
            TablePress::CustomColor(at) => {
                if let Some(color) = (self.manual.custom.as_mut())
                    .and_then(|form| form.colors.get_mut(usize::from(at)))
                {
                    *color = !*color;
                }
                self.show_screen();
            }
            TablePress::CustomCreate => self.create_custom(shift),
            TablePress::CancelCapture => self.stop_capture(None),
            TablePress::Step { at, active } => {
                let Some(view) = self.show.latest().map(|update| &update.view) else {
                    return;
                };
                let id = crate::manual::turn::step_command(view, at, active);
                self.run_table_command(id, crate::manual::Target::Table);
            }
            TablePress::Counter { side, at } => self.adjust_counter(side, at, true),
            TablePress::Quit => {
                self.manual.quitting = false;
                self.requests.push_back(crate::command::Request::Quit);
                self.show_screen();
            }
            TablePress::Next => self.run_table_command("phase.next", crate::manual::Target::Table),
            TablePress::Close => {
                self.manual.close_overlay();
                self.layer.focus(None);
                self.show_screen();
            }
            TablePress::ProposalLine { id, line, on } => {
                self.send_answer(Answer::ProposalLine { id, line, on });
            }
            TablePress::ApplyProposal { id, revision } => {
                self.send_answer(Answer::ApplyProposal { id, revision });
            }
            TablePress::Label(id) => self.send_answer(Answer::ApplyLabel { id }),
            TablePress::SortCard(card) => {
                let view = self.show.latest().map(|update| &update.view);
                if let Some(answer) = view.and_then(|view| assists::sort_answer(view, card)) {
                    self.send_answer(answer);
                }
            }
            TablePress::SortDone => self.send_answer(Answer::Done),
            TablePress::SortCancel => self.send_answer(Answer::Cancel),
            TablePress::Opening(press) => {
                for answer in press.answers() {
                    self.send_answer(answer);
                }
            }
            TablePress::Leave => self.leave_table(),
        }
    }

    /// A key while the command list, ask prompt, token search or order tray is open: Escape
    /// closes it, Return runs the highlighted row (its shifted form with Shift), confirms the
    /// prompt or finishes the order, the up and down keys move a list's highlight, and the left
    /// and right keys move the tray's highlighted card. Returns whether the key means nothing
    /// else; every other key goes on to the focused field.
    pub(super) fn table_overlay_key(&mut self, key: Key) -> bool {
        if !self.manual.overlay_open() || key.phase == KeyPhase::Up {
            return false;
        }
        let first = key.phase == KeyPhase::Down;
        if self
            .manual
            .palette
            .as_ref()
            .is_some_and(|open| open.capturing)
        {
            if first {
                self.capture_key(key);
            }
            return true;
        }
        let shift = key.modifiers.shift;
        match key.code {
            KeyCode::Escape => {
                if first {
                    self.table_press(TablePress::Close);
                }
            }
            KeyCode::Enter => {
                if !first {
                    return true;
                }
                if let Some(open) = &self.manual.palette {
                    if key.modifiers.alt {
                        self.start_capture();
                    } else {
                        self.run_palette_row(open.highlight, shift);
                    }
                } else if self.manual.ask.is_some() {
                    self.confirm_ask();
                } else if let Some(open) = &self.manual.tokens {
                    self.choose_token(open.highlight, shift);
                } else if self.manual.custom.is_some() {
                    self.create_custom(shift);
                } else if self.manual.quitting {
                    self.table_press(TablePress::Quit);
                } else {
                    self.order_done();
                }
            }
            KeyCode::ArrowUp | KeyCode::ArrowDown
                if self.manual.palette.is_some() || self.manual.tokens.is_some() =>
            {
                let down = key.code == KeyCode::ArrowDown;
                let view = self.show.latest().map(|update| &update.view);
                if let Some(open) = &mut self.manual.palette {
                    open.move_highlight(down);
                } else if let (Some(open), Some(view)) = (&mut self.manual.tokens, view) {
                    open.move_highlight(down, view);
                }
                self.show_screen();
            }
            KeyCode::ArrowLeft | KeyCode::ArrowRight if self.manual.order.is_some() => {
                if let Some(order) = &mut self.manual.order {
                    order.nudge(key.code == KeyCode::ArrowRight);
                }
                self.show_screen();
            }
            _ => return false,
        }
        true
    }

    /// Runs the `at`th row the command list shows, or its shifted form if `shift`, and closes
    /// the list.
    fn run_palette_row(&mut self, at: usize, shift: bool) {
        let Some(open) = self.manual.palette.take() else {
            return;
        };
        let query = self.fields.get(palette::FIELD).text;
        let Some(row) = open.matched(&query).get(at).map(|row| (*row).clone()) else {
            self.manual.palette = Some(open);
            return;
        };
        if row.command == RESET {
            self.manual.palette = Some(open);
            self.settings.bindings.reset();
            self.bindings_changed(RESET_NOTICE.to_owned());
            return;
        }
        self.layer.focus(None);
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        let command = &COMMANDS[usize::from(row.command)];
        let act = menus::act(command, row.target, view, &self.table_context(), shift);
        self.give(act, row.target, shift);
        self.show_screen();
    }

    /// Confirms the ask prompt with what its field holds: opens the command's next prompt, or
    /// sends what the command makes and closes the prompt. Nothing while the field holds no
    /// value.
    fn confirm_ask(&mut self) {
        let Some(ask) = &self.manual.ask else {
            return;
        };
        let text = self.fields.get(&ask.field).text;
        self.confirm_ask_with(&text);
    }

    /// Confirms the ask prompt as if its field held `text`.
    fn confirm_ask_with(&mut self, text: &str) {
        let Some(ask) = self.manual.ask.clone() else {
            return;
        };
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        match ask.confirm(text, view) {
            Some(Confirmed::Next(next)) => self.show_ask(*next),
            Some(Confirmed::Send(actions)) => {
                self.manual.ask = None;
                self.layer.focus(None);
                self.send_table(actions);
                self.show_screen();
            }
            Some(Confirmed::Answer(answer)) => {
                self.manual.ask = None;
                self.layer.focus(None);
                self.send_answer(answer);
                self.show_screen();
            }
            None => {}
        }
    }

    /// The ask prompt's plus, or its minus if not `up`: the field takes the next value.
    fn step_ask(&mut self, up: bool) {
        let Some(ask) = &self.manual.ask else {
            return;
        };
        let name = ask.field.clone();
        let text = self.fields.get(&name).text;
        let Some(next) = ask.step(&text, if up { 1 } else { -1 }) else {
            return;
        };
        let end = u32::try_from(next.len()).unwrap_or(0);
        let state = FieldState {
            text: next,
            caret: end,
            selection: None,
            placeholder: String::new(),
        };
        // The field's editor keeps its own text while it is focused, so it takes the new value
        // by being focused again.
        self.layer.focus(None);
        self.fields.set(&name, state);
        self.show_screen();
        self.layer.focus_field(&name);
    }

    /// Makes the token of the `at`th row of the token search, face down if `shift` or if the
    /// search was opened so, and closes the search; the custom token's row asks for its name.
    fn choose_token(&mut self, at: usize, shift: bool) {
        let Some(open) = self.manual.tokens.clone() else {
            return;
        };
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        let face_down = shift || open.face_down;
        match open.choices(view).get(at) {
            Some(Choice::Token { token, .. }) => {
                let card = serde_json::json!({
                    "token": { "name": token.name, "scryfall_id": token.scryfall_id }
                });
                let choice = TokenChoice { card, note: None };
                self.make_token(open.command, &choice, face_down);
            }
            Some(Choice::Custom) => self.open_custom(open.command, face_down),
            None => {}
        }
    }

    /// Sends the action of the token command at `command` of `COMMANDS` that makes `choice`,
    /// face down if `face_down`, and closes the token search or custom token form.
    fn make_token(&mut self, command: u16, choice: &TokenChoice, face_down: bool) {
        let Some(view) = self.show.latest().map(|update| &update.view) else {
            return;
        };
        let mut values = Values::new();
        values.insert("card", choice.card.clone());
        values.insert("text", choice.note.clone().into());
        values.insert("face_down", face_down.into());
        let command = &COMMANDS[usize::from(command)];
        let actions = commands::actions_with(command, Target::Table, view, &values);
        self.manual.tokens = None;
        self.manual.custom = None;
        self.layer.focus(None);
        self.send_table(actions);
        self.show_screen();
    }

    /// Opens the custom token form for the token search's command at `command`, its fields
    /// empty and its name focused, making the token face down if `face_down`.
    fn open_custom(&mut self, command: u16, face_down: bool) {
        self.over(tokens::NAME, FieldState::default());
        for name in tokens::CUSTOM_FIELDS {
            self.fields.set(name, FieldState::default());
        }
        self.manual.custom = Some(CustomToken::open(command, face_down));
        self.show_screen();
        self.layer.focus_field(tokens::NAME);
    }

    /// Makes the token the custom token form describes, face down if `shift` or if the search
    /// was opened so; nothing while the form describes none.
    fn create_custom(&mut self, shift: bool) {
        let Some(form) = self.manual.custom.clone() else {
            return;
        };
        if let Some(choice) = form.choice(&self.fields) {
            self.make_token(form.command, &choice, shift || form.face_down);
        }
    }

    /// Option+Return in the command list: it waits for a key to bind to the highlighted row's
    /// command, with the field no longer taking keys. Nothing on the row that resets the keys,
    /// which no key can be bound to.
    fn start_capture(&mut self) {
        let Some(open) = &mut self.manual.palette else {
            return;
        };
        let query = self.fields.get(palette::FIELD).text;
        let bindable =
            (open.matched(&query).get(open.highlight)).is_some_and(|row| row.command != RESET);
        if !bindable {
            return;
        }
        open.capturing = true;
        self.layer.focus(None);
        self.show_screen();
    }

    /// A key while the command list waits for one: Escape stops waiting, and a key with a name
    /// in a binding is bound to the highlighted row's command, taking it from any command that
    /// had it. A modifier alone keeps the list waiting.
    fn capture_key(&mut self, key: Key) {
        if key.code == KeyCode::Escape {
            self.stop_capture(None);
            return;
        }
        let Some(binding) = binding_for(&key, self.mac()) else {
            return;
        };
        let Some(open) = &self.manual.palette else {
            return;
        };
        let query = self.fields.get(palette::FIELD).text;
        let Some(row) = open
            .matched(&query)
            .get(open.highlight)
            .map(|row| row.command)
        else {
            return;
        };
        let Some(command) = COMMANDS.get(usize::from(row)) else {
            return;
        };
        let mac = self.mac();
        let displaced = self.settings.bindings.bind(command, &binding, mac);
        let label = displaced
            .and_then(commands::command)
            .map(|(_, command)| command.label);
        let notice = label.map_or_else(
            || "Shortcut saved.".to_owned(),
            |label| format!("{label} lost its shortcut."),
        );
        self.stop_capture(Some(notice));
    }

    /// Stops waiting for a key to bind; a binding made says so in `notice`, and the bindings
    /// are saved.
    fn stop_capture(&mut self, notice: Option<String>) {
        if let Some(open) = &mut self.manual.palette {
            open.capturing = false;
        }
        if let Some(notice) = notice {
            self.bindings_changed(notice);
        } else {
            self.show_screen();
            self.layer.focus_field(palette::FIELD);
        }
    }

    /// The player's bindings changed: the command list shows `notice` and the rows' new keys,
    /// its field takes keys again, and the settings are saved.
    fn bindings_changed(&mut self, notice: String) {
        let context = self.table_context();
        let view = self.show.latest().map(|update| &update.view);
        let rows = view.map(|view| {
            let mut rebuilt = self.manual.palette.clone();
            if let Some(open) = &mut rebuilt {
                open.refresh(view, &context);
            }
            rebuilt
        });
        if let Some(Some(mut open)) = rows {
            open.notice = Some(notice);
            self.manual.palette = Some(open);
        }
        self.requests
            .push_back(Request::SaveSettings(self.settings.clone()));
        self.show_screen();
        self.layer.focus_field(palette::FIELD);
    }

    /// Puts the order tray's cards on the library in its order and closes it.
    fn order_done(&mut self) {
        let Some(order) = self.manual.order.take() else {
            return;
        };
        if let Some(view) = self.show.latest().map(|update| &update.view) {
            let actions = order.actions(view);
            self.send_table(actions);
        }
        self.show_screen();
    }
}
