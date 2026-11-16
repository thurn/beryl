//! The manual table's lobby on the client's side (docs/manual-multiplayer.md, "Setup and match
//! flow"), as the web's `Lobby` drives it: the start screen's 'Solo table' opens a solo table's
//! lobby, each control sends a `LobbyChange` as `SessionCommand::TableLobby`, and the session's
//! `Input::Lobby` brings the lobby's new state. The lobby gives way to the table when its game
//! starts, and the table goes back to the lobby when a game ends.

use beryl_protocol::ipc::{LobbyChange, LobbySettings, LobbyUpdate, TableRefused};

use super::Client;
use crate::command::SessionCommand;
use crate::element::LobbyPress;
use crate::manual::{self, lobby};
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
            LobbyPress::Change => {
                let deck = manual::solo_request().deck;
                LobbyChange::LoadDeck {
                    main: deck.main,
                    side: deck.side,
                }
            }
            LobbyPress::Unload if loaded => LobbyChange::LoadDeck {
                main: Vec::new(),
                side: Vec::new(),
            },
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
            LobbyPress::Unload | LobbyPress::Ready | LobbyPress::Start => return,
        };
        self.manual.lobby.error = None;
        self.commands.push_back(SessionCommand::TableLobby(change));
        self.show_screen();
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
