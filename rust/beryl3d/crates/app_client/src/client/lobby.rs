//! Adapts lobby events and replies from the session to feature messages.

use app_screens::screens::{COULD_NOT_OPEN, Screen};
use beryl_protocol::SessionError;
use beryl_protocol::ipc::decks::{DeckImportReply, SavedDeck};
use beryl_protocol::ipc::{LobbyUpdate, TableRefused};
use game_manual::features::{
    context_menu,
    lobby::{EntryMessage, Message},
};

use super::Client;
use crate::command::Reply;

impl Client {
    pub(super) fn ask_saved_tables(&mut self) {
        self.update_lobby(&Message::Entry(EntryMessage::StartVisible(
            self.screen == Screen::Start,
        )));
    }

    pub(super) fn take_entry_reply(&mut self, reply: Reply) {
        let message = match reply {
            Reply::TableSaved(saved) => EntryMessage::SavedTables(saved),
            Reply::TableOpen(Ok(())) => EntryMessage::TableOpened,
            Reply::TableOpen(Err(reason)) => return self.table_failed(reason),
            _ => return,
        };
        self.update_lobby(&Message::Entry(message));
        self.show_screen();
    }

    pub(super) fn table_failed(&mut self, reason: String) {
        if self.opening_table() {
            self.screen = Screen::Start;
        }
        self.update_lobby(&Message::Entry(EntryMessage::EntryError(reason)));
        self.show_screen();
    }

    pub(super) fn table_error(&mut self, reason: String) {
        let message = if self.screen == Screen::Start || self.opening_table() {
            Message::Entry(EntryMessage::EntryError(reason))
        } else {
            Message::LinkError(Some(reason))
        };
        self.update_lobby(&message);
        self.show_screen();
    }

    pub(super) fn take_lobby_reply(
        &mut self,
        reply: Result<Option<Box<LobbyUpdate>>, TableRefused>,
    ) {
        match reply {
            Ok(Some(update)) => self.take_lobby(*update),
            Ok(None) => {}
            Err(_) if self.opening_table() => self.table_failed(COULD_NOT_OPEN.to_owned()),
            Err(_) => {
                self.update_lobby(&Message::Refused);
                self.show_screen();
            }
        }
    }

    pub(super) fn take_lobby(&mut self, update: LobbyUpdate) {
        match update.view {
            Some(view) => {
                if view.seats.len() > 1 && view.seats.iter().all(|seat| seat.name.is_some()) {
                    self.update_lobby(&Message::LinkError(None));
                }
                self.features.lobby_view = Some(view);
                if self.screen == Screen::Table {
                    self.update_table_menu(&context_menu::Message::Close);
                    self.screen = Screen::Lobby;
                }
            }
            None if self.screen == Screen::Lobby => self.screen = Screen::Table,
            None => {}
        }
        self.show_screen();
    }

    pub(super) fn take_saved_decks(&mut self, saved: Result<Vec<SavedDeck>, SessionError>) {
        if self.update_lobby(&Message::SavedDecks(saved)) {
            self.show_screen();
        }
    }

    pub(super) fn take_imported_deck(&mut self, imported: Result<DeckImportReply, SessionError>) {
        if self.update_lobby(&Message::ImportedDeck(imported)) {
            self.show_screen();
        }
    }

    fn opening_table(&self) -> bool {
        self.screen == Screen::Lobby && self.features.lobby_view.is_none()
    }
}
