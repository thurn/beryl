//! Opening and resuming tables from the start screen.

use super::{Command, Effects, Request, State};
use crate::manual::lobby::Lobby;
use crate::screens::table_entries::EntryMode;
use beryl_protocol::ipc::{LobbyChange, SavedTable, TableLink};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryMessage {
    /// Open a solo table's lobby, from the start screen.
    Solo,
    /// Open the start screen's form for hosting a table.
    Host,
    /// Toggle localhost hosting in the host form.
    LocalOnly,
    /// Open the start screen's form for joining a table.
    Join,
    /// Close the start screen's host or join form.
    CancelEntry,
    /// Host or join a table with what the open form holds.
    OpenTable {
        name: String,
        invite: String,
    },
    /// Resume the saved table at this index of the start screen's list.
    Resume(u16),
    /// The start screen mounted or unmounted.
    StartVisible(bool),
    SavedTables(Result<Vec<SavedTable>, String>),
    TableOpened,
    EntryError(String),
}

pub(super) fn update(state: &mut State, message: &EntryMessage, effects: &mut Effects<'_>) -> bool {
    let entries = &mut state.entries;
    match message {
        EntryMessage::Host | EntryMessage::Join | EntryMessage::CancelEntry => {
            entries.mode = match message {
                EntryMessage::Host => Some(EntryMode::Host),
                EntryMessage::Join => Some(EntryMode::Join),
                _ => None,
            };
            entries.error = None;
        }
        EntryMessage::LocalOnly => entries.local_only = !entries.local_only,
        EntryMessage::OpenTable { name, invite } => {
            let command = match entries.mode {
                Some(EntryMode::Host) if !name.is_empty() => Command::Host {
                    name: name.clone(),
                    link: if entries.local_only {
                        TableLink::Local
                    } else {
                        TableLink::Iroh
                    },
                },
                Some(EntryMode::Join) if !name.is_empty() && !invite.is_empty() => Command::Join {
                    invite: invite.clone(),
                    name: name.clone(),
                },
                _ => return false,
            };
            open(state, effects, command);
        }
        EntryMessage::Resume(at) => {
            let Some(saved) = entries.saved.get(usize::from(*at)) else {
                return false;
            };
            let id = saved.id.clone();
            open(state, effects, Command::Resume(id));
        }
        EntryMessage::Solo => open(state, effects, Command::Change(LobbyChange::Solo)),
        EntryMessage::StartVisible(visible) => {
            if *visible && !entries.asked {
                effects.command(Command::SavedTables);
            }
            let changed = entries.asked != *visible;
            entries.asked = *visible;
            return changed;
        }
        EntryMessage::SavedTables(result) => {
            entries.saved = result.as_ref().cloned().unwrap_or_default();
            entries.saved_error = result.is_err();
        }
        EntryMessage::TableOpened => {
            entries.mode = None;
            // The lobby's first state may arrive before this answer.
            effects.command(Command::Change(LobbyChange::View));
        }
        EntryMessage::EntryError(error) => entries.error = Some(error.clone()),
    }
    true
}

fn open(state: &mut State, effects: &mut Effects<'_>, command: Command) {
    state.entries.error = None;
    state.lobby = Lobby::new();
    effects.command(command);
    effects.request(Request::OpenLobby);
}
