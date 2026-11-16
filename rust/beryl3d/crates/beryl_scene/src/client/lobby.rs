//! The manual table's lobby on the client's side (docs/manual-multiplayer.md, "Setup and match
//! flow"), as the web's `Lobby` drives it: the start screen's 'Solo table' opens a solo table's
//! lobby, each control sends a `LobbyChange` as `SessionCommand::TableLobby`, and the session's
//! `Input::Lobby` brings the lobby's new state. The deck picker asks the session for the saved
//! decks (`SessionCommand::DeckList`) or to read a pasted decklist
//! (`SessionCommand::DeckImport`), and loads the deck it gets back. The lobby gives way to the table when its game
//! starts, and the table goes back to the lobby when a game ends.

use beryl_protocol::SessionError;
use beryl_protocol::ipc::decks::{DeckImportReply, DeckImportRequest, SavedDeck};
use beryl_protocol::ipc::{LobbyChange, LobbySettings, LobbyUpdate, TableRefused};

use super::Client;
use crate::command::SessionCommand;
use crate::element::LobbyPress;
use crate::manual::lobby::{self, DeckPanel};
use crate::screens::Screen;
use crate::widgets::FieldState;

impl Client {
    /// Carries out a press of one of the lobby's controls, or the start screen's 'Solo table'.
    pub(super) fn press_lobby(&mut self, press: LobbyPress) {
        let lobby = &self.manual.lobby;
        let view = lobby.view.as_ref();
        let loaded = lobby.own().is_some_and(|own| own.loaded);
        let change = match press {
            LobbyPress::Solo => {
                self.open_lobby();
                LobbyChange::Solo
            }
            LobbyPress::Change => return self.open_picker(DeckPanel::Saved),
            LobbyPress::Paste => return self.open_picker(DeckPanel::Paste),
            LobbyPress::ClosePicker => {
                self.manual.lobby.picker = None;
                return self.show_screen();
            }
            LobbyPress::Pick(at) => {
                let Some(saved) = lobby.saved.get(usize::from(at)) else {
                    return;
                };
                return self.load_deck(saved.deck.clone());
            }
            LobbyPress::Import => return self.import_deck(),
            LobbyPress::Sideboard if loaded => {
                self.manual.lobby.sideboard = !lobby.sideboard;
                return self.show_screen();
            }
            LobbyPress::Swap { from_side, at } => {
                let Some(change) = view.and_then(|view| lobby::swapped(view, from_side, at)) else {
                    return;
                };
                change
            }
            LobbyPress::Unload if loaded => {
                self.manual.lobby.deck_name = None;
                self.manual.lobby.sideboard = false;
                LobbyChange::LoadDeck {
                    main: Vec::new(),
                    side: Vec::new(),
                }
            }
            LobbyPress::Ready if loaded && !lobby.hosting() => LobbyChange::Ready {
                ready: !lobby.own().is_some_and(|own| own.ready),
            },
            LobbyPress::Assist(at) => {
                let (Some(view), Some((assist, _))) = (view, lobby::ASSISTS.get(usize::from(at)))
                else {
                    return;
                };
                lobby::toggled(view, *assist)
            }
            LobbyPress::SaveSettings => {
                let Some(settings) = view.and_then(|view| self.asked_settings(view.settings))
                else {
                    return;
                };
                // The fields empty again, to show the settings the lobby comes back with.
                self.fields.set(lobby::STARTING_LIFE, FieldState::default());
                self.fields
                    .set(lobby::STARTING_HAND_SIZE, FieldState::default());
                LobbyChange::Settings { settings }
            }
            LobbyPress::Start if view.is_some_and(|view| view.can_start) => LobbyChange::Start,
            LobbyPress::Leave => return self.leave_table(),
            LobbyPress::Sideboard | LobbyPress::Unload | LobbyPress::Ready | LobbyPress::Start => {
                return;
            }
        };
        self.send_lobby(change);
    }

    /// Sends the lobby `change` and shows the lobby until the table answers.
    fn send_lobby(&mut self, change: LobbyChange) {
        self.manual.lobby.error = None;
        self.commands.push_back(SessionCommand::TableLobby(change));
        self.show_screen();
    }

    /// Opens the deck picker on `panel`. The saved decks' panel asks the session for them.
    fn open_picker(&mut self, panel: DeckPanel) {
        let lobby = &mut self.manual.lobby;
        lobby.picker = Some(panel);
        lobby.deck_error = None;
        if panel == DeckPanel::Saved {
            lobby.saved.clear();
            self.commands.push_back(SessionCommand::DeckList);
        }
        self.show_screen();
    }

    /// Sends the pasted decklist to be read and saved under the name typed, once both fields
    /// hold something and no decklist is being read already.
    fn import_deck(&mut self) {
        let name = self.fields.get(lobby::DECK_NAME).text.trim().to_owned();
        let text = self.fields.get(lobby::DECKLIST).text;
        if name.is_empty() || text.trim().is_empty() || self.manual.lobby.importing {
            return;
        }
        self.manual.lobby.importing = true;
        self.manual.lobby.deck_error = None;
        (self.commands).push_back(SessionCommand::DeckImport(DeckImportRequest {
            name,
            text: Some(text),
            cod: None,
        }));
        self.show_screen();
    }

    /// Loads `deck` at the table, keeps its name to show, and closes the picker.
    fn load_deck(&mut self, deck: DeckImportReply) {
        self.fields.set(lobby::DECK_NAME, FieldState::default());
        self.fields.set(lobby::DECKLIST, FieldState::default());
        let lobby = &mut self.manual.lobby;
        lobby.deck_name = Some(deck.name);
        lobby.picker = None;
        lobby.importing = false;
        lobby.sideboard = false;
        self.send_lobby(LobbyChange::LoadDeck {
            main: deck.main,
            side: deck.side,
        });
    }

    /// The session listed the saved decks, which the picker shows if it is still open on them.
    pub(super) fn take_saved_decks(&mut self, saved: Result<Vec<SavedDeck>, SessionError>) {
        let lobby = &mut self.manual.lobby;
        if lobby.picker != Some(DeckPanel::Saved) {
            return;
        }
        match saved {
            Ok(saved) => lobby.saved = saved,
            Err(error) => lobby.deck_error = Some(lobby::deck_error(&error)),
        }
        self.show_screen();
    }

    /// The session read a pasted decklist and saved it: the table loads it. Or it could not,
    /// and the picker says why.
    pub(super) fn take_imported_deck(&mut self, imported: Result<DeckImportReply, SessionError>) {
        if !self.manual.lobby.importing {
            return;
        }
        match imported {
            Ok(deck) => self.load_deck(deck),
            Err(error) => {
                let lobby = &mut self.manual.lobby;
                lobby.importing = false;
                lobby.deck_error = Some(lobby::deck_error(&error));
                self.show_screen();
            }
        }
    }

    /// The settings the host's fields ask for, if they can be read and ask for a change.
    fn asked_settings(&self, current: LobbySettings) -> Option<LobbySettings> {
        let life = self.fields.get(lobby::STARTING_LIFE).text;
        let hand = self.fields.get(lobby::STARTING_HAND_SIZE).text;
        lobby::asked_settings(current, &life, &hand).ok().flatten()
    }

    /// Shows the lobby screen with no lobby yet, which arrives as `Input::Lobby`.
    fn open_lobby(&mut self) {
        self.first_view_opens = None;
        self.manual.lobby = lobby::Lobby::new();
        self.screen = Screen::Lobby;
        self.sheet = None;
    }

    /// The lobby changed. A lobby with a state is kept, and shown in place of the table when a
    /// game there has ended. No state means the table's game has started: the lobby gives way
    /// to the table.
    pub(super) fn take_lobby(&mut self, update: LobbyUpdate) {
        match update.view {
            Some(view) => {
                self.manual.lobby.view = Some(view);
                if self.screen == Screen::Table {
                    self.manual.menu = None;
                    self.screen = Screen::Lobby;
                }
            }
            None if self.screen == Screen::Lobby => self.screen = Screen::Table,
            None => {}
        }
        self.show_screen();
    }

    /// The table refused a change of its lobby: the lobby says so, as the web's does.
    pub(super) fn lobby_refused(&mut self, _: &TableRefused) {
        self.manual.lobby.error = Some("Could not update the lobby.");
        self.show_screen();
    }
}
