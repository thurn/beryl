//! The table lobby, deck picker and start screen's table-entry controls.

mod entries;
mod view;

pub use entries::EntryMessage;

use beryl_protocol::SessionError;
use beryl_protocol::ipc::decks::{DeckImportReply, DeckImportRequest, SavedDeck};
use beryl_protocol::ipc::{LobbyChange, LobbySettings};
use kit_scene::{Context, Feature};
use kit_ui::element::Element;

use crate::manual::lobby::{self, DeckPanel, Lobby};
use crate::screens::table_entries::TableEntries;

pub use view::{Inputs, SHORTCUTS, shortcut};

/// The lobby and table-entry controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Entry(EntryMessage),
    /// Open the deck picker on the saved decks ('Change').
    Change,
    /// Open the deck picker on the field a decklist is pasted into ('Paste decklist').
    Paste,
    /// Load the saved deck at this index of the deck picker's list.
    Pick(u16),
    /// Read the pasted decklist, save it under the name typed and load it ('Import').
    Import {
        name: String,
        text: String,
    },
    /// Close the deck picker.
    ClosePicker,
    /// Open the sideboard swap, or close it.
    Sideboard,
    /// Unload the loaded deck.
    Unload,
    /// A change computed from the session's current lobby view.
    ChangeLobby(LobbyChange),
    /// Ask for the starting life and hand size the host's fields hold.
    SaveSettings(Option<LobbySettings>),
    /// Leave the table and go back to the start screen.
    Leave,
    LinkError(Option<String>),
    Refused,
    SavedDecks(Result<Vec<SavedDeck>, SessionError>),
    ImportedDeck(Result<DeckImportReply, SessionError>),
    Reset,
}

#[derive(Debug, Default)]
pub struct State {
    pub lobby: Lobby,
    pub entries: TableEntries,
}

pub enum Command {
    Change(LobbyChange),
    SavedTables,
    Host(String),
    Join { invite: String, name: String },
    Resume(String),
    SavedDecks,
    Import(DeckImportRequest),
}

pub enum Request {
    OpenLobby,
    Leave,
    ClearFields(&'static [&'static str]),
}

type Effects<'a> = Context<'a, Command, Request>;

pub struct LobbyScreen;

impl Feature for LobbyScreen {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = view::ViewKey;
    type Command = Command;
    type Request = Request;

    fn update(state: &mut State, message: &Message, effects: &mut Effects<'_>) -> bool {
        let lobby = &mut state.lobby;
        match message {
            Message::Entry(message) => return entries::update(state, message, effects),
            Message::Change | Message::Paste => {
                let saved = matches!(message, Message::Change);
                lobby.picker = Some(if saved {
                    DeckPanel::Saved
                } else {
                    DeckPanel::Paste
                });
                lobby.deck_error = None;
                if saved {
                    lobby.saved.clear();
                    effects.command(Command::SavedDecks);
                }
            }
            Message::ClosePicker => lobby.picker = None,
            Message::Pick(at) => {
                let Some(saved) = lobby.saved.get(usize::from(*at)) else {
                    return false;
                };
                load_deck(lobby, effects, saved.deck.clone());
            }
            Message::Import { name, text }
                if !name.is_empty() && !text.trim().is_empty() && !lobby.importing =>
            {
                lobby.importing = true;
                lobby.deck_error = None;
                effects.command(Command::Import(DeckImportRequest {
                    name: name.clone(),
                    text: Some(text.clone()),
                    cod: None,
                }));
            }
            Message::Sideboard => lobby.sideboard = !lobby.sideboard,
            Message::ChangeLobby(change) => send(lobby, effects, change.clone()),
            Message::Unload => {
                lobby.deck_name = None;
                lobby.sideboard = false;
                send(
                    lobby,
                    effects,
                    LobbyChange::LoadDeck {
                        main: Vec::new(),
                        side: Vec::new(),
                    },
                );
            }
            Message::SaveSettings(Some(settings)) => {
                effects.request(Request::ClearFields(&[
                    lobby::STARTING_LIFE,
                    lobby::STARTING_HAND_SIZE,
                ]));
                send(
                    lobby,
                    effects,
                    LobbyChange::Settings {
                        settings: *settings,
                    },
                );
            }
            Message::Leave => {
                effects.request(Request::Leave);
                return false;
            }
            Message::LinkError(error) => lobby.link_error.clone_from(error),
            Message::Refused => lobby.error = Some("Could not update the lobby."),
            Message::SavedDecks(saved) if lobby.picker == Some(DeckPanel::Saved) => match saved {
                Ok(saved) => lobby.saved.clone_from(saved),
                Err(error) => lobby.deck_error = Some(lobby::deck_error(error)),
            },
            Message::ImportedDeck(imported) if lobby.importing => match imported {
                Ok(deck) => load_deck(lobby, effects, deck.clone()),
                Err(error) => {
                    lobby.importing = false;
                    lobby.deck_error = Some(lobby::deck_error(error));
                }
            },
            Message::Reset => *lobby = Lobby::new(),
            Message::Import { .. }
            | Message::SavedDecks(_)
            | Message::ImportedDeck(_)
            | Message::SaveSettings(None) => {
                return false;
            }
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs<'_>) -> Self::ViewKey {
        view::key(inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(&state.lobby, inputs)
    }
}

fn send(lobby: &mut Lobby, effects: &mut Effects<'_>, change: LobbyChange) {
    lobby.error = None;
    effects.command(Command::Change(change));
}

fn load_deck(lobby: &mut Lobby, effects: &mut Effects<'_>, deck: DeckImportReply) {
    effects.request(Request::ClearFields(&[lobby::DECK_NAME, lobby::DECKLIST]));
    lobby.deck_name = Some(deck.name);
    lobby.picker = None;
    lobby.importing = false;
    lobby.sideboard = false;
    send(
        lobby,
        effects,
        LobbyChange::LoadDeck {
            main: deck.main,
            side: deck.side,
        },
    );
}
