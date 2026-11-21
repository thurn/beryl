//! Hosting, joining and resuming a manual table from the start screen, as the web's
//! `TableEntries` does (`ui/src/table/lobby/TableEntries.tsx`). The start screen asks the
//! session for the saved tables each time it opens (`Client::ask_saved_tables`). Opening a
//! table shows the lobby at once, empty until its state arrives, as 'Solo table' does; a table
//! that does not open puts the start screen back with the session's reason under its form.
//! A failure the table reports later (`Input::TableError`), such as a guest with another
//! version of Beryl, shows on the lobby and under the table's connection banner.

use beryl_protocol::ipc::{LobbyChange, LobbyUpdate, TableRefused};
use game_manual::manual::LobbyPress;

use super::Client;
use crate::command::{Reply, SessionCommand};

use app_screens::screens::{EntryMode, INVITE, NAME, Screen};

impl Client {
    /// Carries out a press of the start screen's table entries: open or close a form, open the
    /// table the form describes, or resume a saved table.
    pub(super) fn press_entry(&mut self, press: LobbyPress) {
        let entries = &mut self.manual.entries;
        match press {
            LobbyPress::Host => entries.mode = Some(EntryMode::Host),
            LobbyPress::Join => entries.mode = Some(EntryMode::Join),
            LobbyPress::CancelEntry => entries.mode = None,
            LobbyPress::OpenTable => return self.open_from_form(),
            LobbyPress::Resume(at) => {
                if let Some(saved) = entries.saved.get(usize::from(at)) {
                    let id = saved.id.clone();
                    return self.open_table(SessionCommand::TableResume(id));
                }
                return;
            }
            _ => return,
        }
        entries.error = None;
        self.show_screen();
    }

    /// Hosts or joins a table with the open form's name and invite code, once the fields hold
    /// what it needs.
    fn open_from_form(&mut self) {
        let name = self.fields.get(NAME).text.trim().to_owned();
        let invite = self.fields.get(INVITE).text.trim().to_owned();
        let command = match self.manual.entries.mode {
            Some(EntryMode::Host) if !name.is_empty() => SessionCommand::TableHost { name },
            Some(EntryMode::Join) if !name.is_empty() && !invite.is_empty() => {
                SessionCommand::TableJoin { invite, name }
            }
            _ => return,
        };
        self.open_table(command);
    }

    /// Sends `command`, which opens a table, and shows the lobby until the table answers.
    fn open_table(&mut self, command: SessionCommand) {
        self.manual.entries.error = None;
        self.commands.push_back(command);
        self.open_lobby();
        self.show_screen();
    }

    /// Asks the session for the saved tables when the start screen has just opened, once each
    /// time it opens, as the web's `TableEntries` asks when it mounts.
    pub(super) fn ask_saved_tables(&mut self) {
        let entries = &mut self.manual.entries;
        if self.screen != Screen::Start {
            entries.asked = false;
        } else if !entries.asked {
            entries.asked = true;
            self.commands.push_back(SessionCommand::TableSaved);
        }
    }

    /// The session's answer to opening a table or to listing the saved tables.
    pub(super) fn take_entry_reply(&mut self, reply: Reply) {
        let entries = &mut self.manual.entries;
        match reply {
            Reply::TableSaved(Ok(saved)) => {
                entries.saved = saved;
                entries.saved_error = false;
            }
            Reply::TableSaved(Err(_)) => {
                entries.saved.clear();
                entries.saved_error = true;
            }
            // The lobby's first state may have come as an event before this answer; asking for
            // it again shows it either way.
            Reply::TableOpen(Ok(())) => {
                entries.mode = None;
                (self.commands).push_back(SessionCommand::TableLobby(LobbyChange::View));
            }
            Reply::TableOpen(Err(reason)) => return self.table_failed(reason),
            _ => return,
        }
        self.show_screen();
    }

    /// The table did not open, for `reason`: the start screen comes back, with its form as it
    /// was, and says why.
    pub(super) fn table_failed(&mut self, reason: String) {
        if self.screen == Screen::Lobby && self.manual.lobby.view.is_none() {
            self.screen = Screen::Start;
        }
        self.manual.entries.error = Some(reason);
        self.show_screen();
    }

    /// The table reported a failure. While a table is still opening it is why the table did
    /// not open, which the start screen says; the session's answer to the command that opened
    /// it brings the start screen back. Once the table is open, the lobby and the table show it.
    pub(super) fn table_error(&mut self, reason: String) {
        let opening = self.screen == Screen::Start
            || (self.screen == Screen::Lobby && self.manual.lobby.view.is_none());
        if opening {
            self.manual.entries.error = Some(reason);
        } else {
            self.manual.lobby.link_error = Some(reason);
        }
        self.show_screen();
    }

    /// The session's answer to a change of the lobby: the lobby asked for, or a refusal.
    pub(super) fn take_lobby_reply(
        &mut self,
        lobby: Result<Option<Box<LobbyUpdate>>, TableRefused>,
    ) {
        match lobby {
            Ok(Some(update)) => self.take_lobby(*update),
            Ok(None) => {}
            Err(refused) => self.lobby_refused(&refused),
        }
    }
}
